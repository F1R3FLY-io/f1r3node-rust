//! The set discipline of the native spaces (P5a, P5 design v3, A.7 and A.12). The agent
//! `GenericSpace<HashMapOuter, SetDiscipline>` runs directly over an in-memory play space and its
//! replay space.
//!
//! The example tests are the set tests of the P5 design v2 (section 2.8.4) with the changes of
//! v3 A.12. `peek_retires_nothing` became `space_set_discipline_peek_restores_representative`.
//! The suite adds `peek_restore_reoffer_retires_class`, `members_in_canonical_order`,
//! `legacy_charges_equal_host`, `reservation_cut_rejects_without_effect` and
//! `persistent_reissue_keeps_identity`.
//!
//! The property tests check the headlines of `SetDiscipline.v` and the invariant V6 of
//! `GenericRSpace.tla` on the agent:
//!
//! | Headline or invariant | Property |
//! |---|---|
//! | F6 `set_key_injective` | `prop_set_key_injective` |
//! | F7 `set_view_dedups_equal_pars` | `prop_set_view_dedups_equal_pars` |
//! | F8 `set_consume_retires_class` | `prop_set_consume_retires_class` |
//! | F9 `set_retire_comm_lines_up` | `prop_set_retire_comm_lines_up` |
//! | F10 `set_produce_fire_retires_stored_class` | `prop_set_produce_fire_retires_stored_class` |
//! | `set_peek_restores_representative_only` | `prop_set_peek_restores_representative_only` |
//! | `set_membership_full_compare_on_equal_hash` | `prop_set_membership_full_compare_on_equal_hash` |
//! | `set_key_trace_covered` | `prop_set_key_trace_covered` |
//! | V6 `SetClassRetired` | `prop_set_class_retired_any_schedule` |
//! | Option P (decision 45) | `prop_set_legacy_charges_equal_host` |
//!
//! The host differential runs the equivalent host program on a real `ChargingRSpace`. The
//! program makes the host call of each staged operation of the agent. It also makes the calls
//! of the reducer after a fire: the re-produce of a persistent send, the re-consume of a
//! persistent receive, and `produce_peeks`. A class fire is the host consume over `[D; n]`. A
//! K_retire is the host consume of the retire continuation over `[D]`. The host matcher matches
//! the payload of a record without its envelope, as the agent does. No host program chooses a
//! class, so the test pins the host matcher to the records that the set semantics binds. The
//! charges of each call are then the charges of `ChargingRSpace` for that call.

#[path = "support/mod.rs"]
mod support;

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::cost_signature::Value as CostSignatureValue;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{
    BindPattern, CostAuthority, CostRegion, CostSignature, CostStack, EEq, EList, EVar, Expr,
    ListParWithRandom, Par, TaggedContinuation, Var,
};
use models::rust::host_work::{HostWorkDimension, HostWorkLimit, HostWorkLimits};
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;
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
use rholang::rust::interpreter::host_work::HostWorkBudget;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::reified::costs::{ReissueCause, ScanMeter, FIRE_ROUND_CAP};
use rholang::rust::interpreter::rho_runtime::RhoISpace;
use rholang::rust::interpreter::spaces::agent::{
    ConsumeOp, ProduceOp, SpaceEnv, SpaceOp, SpaceOutcome, SpaceReissue,
};
use rholang::rust::interpreter::spaces::disciplines::bag::BagDiscipline;
use rholang::rust::interpreter::spaces::disciplines::scan::{retire_continuation, retire_pattern};
use rholang::rust::interpreter::spaces::disciplines::set::{set_key, SetDiscipline, LENGTH_BYTES};
use rholang::rust::interpreter::spaces::errors::SpaceError;
use rholang::rust::interpreter::spaces::generic::GenericSpace;
use rholang::rust::interpreter::spaces::namespace::{
    Durability, NativeSpace, RandomPurpose, SID_LEN,
};
use rholang::rust::interpreter::spaces::outer::hashmap::HashMapOuter;
use rholang::rust::interpreter::spaces::record;
use rholang::rust::interpreter::spaces::types::qualifier::SpaceQualifier;
use rholang::rust::interpreter::storage::charging_rspace::ChargingRSpace;
use rspace_plus_plus::rspace::checkpoint::Checkpoint;
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider;
use rspace_plus_plus::rspace::internal::Datum;
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::space_store_handle::{canonical_data_order, metered_hash};
use rspace_plus_plus::rspace::trace::event::{Consume, Event, IOEvent, Produce, COMM};
use support::agent::{
    any, body_name, budget, config_of, counter, counter_channel, create, creation, data_channel,
    env_over, exactly, fired, int, play, play_and_replay, receive_on, run, send_on,
    space_counter_channel, string, unbounded_host_work, ParRSpace,
};
use support::legacy::{host_consume, host_produce, ParResult};
use support::props::{check, pure_config, tokio_runtime};

// ─── The rig ─────────────────────────────────────────────────────────────────────────────

/// The set agent.
type Set = GenericSpace<HashMapOuter, SetDiscipline>;

/// The bag agent, for the negative control of the class key.
type Bag = GenericSpace<HashMapOuter, BagDiscipline>;

/// The space of every rig.
const SPACE: NativeSpace = NativeSpace {
    sid: [23; SID_LEN],
    durability: Durability::Durable,
};

/// The phlo of a legacy differential: far above every program of the suite.
const LEGACY_PHLO: i64 = 1_000_000_000_000;

/// The dimensions that a space agent charges.
const AGENT_DIMENSIONS: [HostWorkDimension; 3] = [
    HostWorkDimension::VerificationOperations,
    HostWorkDimension::VerificationBytes,
    HostWorkDimension::SearchStateBytes,
];

/// The family `hashmap:set`.
fn set_family() -> SpaceFactoryUrn {
    SpaceFactoryUrn {
        outer: SpaceOuter::HashMap,
        inner: SpaceInner::Set,
    }
}

/// The channel of the collection of every rig.
fn channel() -> Par { string("rrs-set") }

/// The data channel `D(sid,k)` of the collection.
fn data() -> Par { data_channel(SPACE, &channel()) }

/// A set space over a fresh in-memory play space.
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
        let config = config_of(SPACE, "hashmap", "set", SpaceQualifier::Default);
        create(&Set::default(), &env, creation(SPACE, config, set_family())).await;
        host.take_event_log().await;
        Rig { host, env }
    }

    /// The same rig with a meter that charges `host_work`.
    fn metered(self, host_work: &HostWorkBudget) -> Rig {
        Rig {
            host: self.host,
            env: SpaceEnv {
                meter: ScanMeter::new(Some(host_work.clone())),
                ..self.env
            },
        }
    }

    async fn run(&self, op: SpaceOp) -> Result<SpaceOutcome, SpaceError> {
        run(&Set::default(), &self.env, op).await
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

    /// The stored records of the collection.
    async fn records(&self) -> Vec<Datum<ListParWithRandom>> { self.host.get_data(&data()).await }

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

/// The value of the payload of a stored record.
fn record_value(record: &ListParWithRandom) -> i64 {
    value_of(
        &record::unwrap_datum(record)
            .expect("a datum record")
            .datum
            .pars,
    )
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
            assert_eq!(record.ord, 0, "every record of a set has the ord 0");
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
            assert_eq!(parked.ord, 0, "every waiter of a set has the ord 0");
            body_name(&parked.continuation)
        })
        .collect();
    names.sort();
    names
}

/// The events of a log as labels: `produce`, `consume`, `comm` or `comm peek`. Each label
/// names each channel of its event: `D` for the data channel of the collection, and `other` for
/// any other channel.
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

/// The consume events of a log.
fn consumes(log: &[Event]) -> Vec<Consume> {
    log.iter()
        .filter_map(|event| match event {
            Event::IoEvent(IOEvent::Consume(consume)) => Some(consume.clone()),
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

/// The record that the one result of the fire `index` of `outcome` removed.
fn removed(outcome: &SpaceOutcome, index: usize) -> ListParWithRandom {
    match outcome.fires.get(index).map(|fire| fire.results.as_slice()) {
        Some([result]) => result.removed_datum.clone(),
        other => panic!("fire {index} has one result: {other:?}"),
    }
}

// ─── Operations and the canonical order ───────────────────────────────────────────────────

/// A send of `value` whose random state comes from `salt`.
fn salted(value: i64, salt: u64, persistent: bool) -> ProduceOp {
    let mut op = send_on(SPACE, &channel(), value, persistent);
    op.data.random_state =
        Blake2b512Random::create_from_bytes(format!("rrs-set-send {value} {salt}").as_bytes())
            .to_bytes();
    op
}

/// A send of the payload `pars` whose random state comes from `salt`.
fn send_pars(pars: Vec<Par>, salt: u64) -> ProduceOp {
    let mut op = salted(0, salt, false);
    op.data.random_state =
        Blake2b512Random::create_from_bytes(format!("rrs-set-pars {pars:?} {salt}").as_bytes())
            .to_bytes();
    op.data.pars = pars;
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
        format!("rrs-set-receive {} {salt}", body_name(&parked_body(&op))).as_bytes(),
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

/// The pattern that matches exactly the payload `pars`.
fn literal(pars: Vec<Par>) -> BindPattern {
    BindPattern {
        patterns: pars,
        remainder: None,
        free_count: 0,
    }
}

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

/// The consume of K_retire of the stored record `member` after a fire of the element `element`,
/// a datum record. Its retire continuation has the source hash of the member as its body. It
/// has the random state of the element under `RandomPurpose::Retire` and the cost authority of
/// the element.
fn retire_consume(element: &ListParWithRandom, member: &Blake2b256Hash) -> Consume {
    let random = RandomPurpose::Retire.derive(&Blake2b512Random::from_bytes(&element.random_state));
    Consume::create(
        &vec![data()],
        &vec![retire_pattern()],
        &retire_continuation(member, &random, element.cost_authority.clone()),
        false,
    )
}

/// The set key of the payload of a stored record.
fn key_of_record(record: &ListParWithRandom) -> Vec<u8> {
    set_key(
        &record::unwrap_datum(record)
            .expect("a datum record")
            .datum
            .pars,
        &ScanMeter::new(None),
    )
    .expect("a key")
}

/// The records of the collection on `space` in canonical order.
async fn canonical_records(
    space: &impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
) -> Vec<Datum<ListParWithRandom>> {
    let records = space.get_data(&data()).await;
    canonical_data_order(&records, &ScanMeter::new(None))
        .expect("an order")
        .into_iter()
        .map(|index| records[index].clone())
        .collect()
}

// ─── The model (SetDiscipline.v) ──────────────────────────────────────────────────────────

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
enum SetOp {
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

impl SetOp {
    fn space_op(&self) -> SpaceOp {
        match *self {
            SetOp::Send {
                value,
                salt,
                persistent,
            } => SpaceOp::Produce(salted(value, salt, persistent)),
            SetOp::Receive {
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

/// What one operation does in the model: its fires, its K_retire COMMs and the classes of its
/// fires that do not peek.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Predicted {
    fires: Vec<(String, Vec<Par>)>,
    retired: usize,
    classes: BTreeSet<i64>,
}

/// The executable model of a set collection: the plans of `SetDiscipline.v` over values. The
/// class of a value is the value, because every generated payload is one number. The candidates
/// of each side are visited by source hash (decision 41). A consume checks the guard once on
/// its first spatial match (decision 37). It binds the whole class unless it peeks (F3). A
/// produce skips a waiter whose guard refuses. A fire of a waiter that does not peek retires the
/// stored non-persistent members of the class of the datum (decision 37). A peek of a
/// non-persistent record removes it and offers it again (decision 45, Rule E). The generators
/// never make a divergent pair.
#[derive(Clone, Debug, Default)]
struct SetModel {
    records: Vec<ModelRecord>,
    waiters: Vec<ModelWaiter>,
}

impl SetModel {
    /// Applies `op` and returns what it does.
    fn apply(&mut self, op: &SetOp) -> Predicted {
        let mut out = Predicted::default();
        match (op, op.space_op()) {
            (
                SetOp::Send {
                    value, persistent, ..
                },
                SpaceOp::Produce(send),
            ) => self.offer(*value, *persistent, record_hash(&send), &mut out),
            (
                SetOp::Receive {
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
                self.receive(waiter, &mut out)
            }
            _ => panic!("an operation and its space operation disagree"),
        }
        out
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

    /// RETIRE-CLASS of `value`: removes the stored non-persistent members of the class.
    fn retire_class(&mut self, value: i64, out: &mut Predicted) {
        let before = self.records.len();
        self.records
            .retain(|record| record.value != value || record.persist);
        out.retired += before - self.records.len();
    }

    /// OFFER-SCAN: the datum fires the first waiter in canonical order that it fits and whose
    /// guard admits it. It fires again while it is not spent, and it is stored when no waiter
    /// takes it. A fire of a waiter that does not peek retires the class of the datum.
    fn offer(&mut self, value: i64, persist: bool, hash: Blake2b256Hash, out: &mut Predicted) {
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
            out.fires
                .push((waiter.name.clone(), waiter.pat.bound(value)));
            if !waiter.persist {
                self.waiters.remove(index);
            }
            if !waiter.peek {
                out.classes.insert(value);
                self.retire_class(value, out);
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

    /// RECEIVE-SCAN: the first record in canonical order that the pattern fits, and the guard
    /// checked once on it. A refusal parks the receive. A receive that does not peek takes the
    /// whole class, and a peeked record is offered again.
    fn receive(&mut self, waiter: ModelWaiter, out: &mut Predicted) {
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
            out.fires
                .push((waiter.name.clone(), waiter.pat.bound(record.value)));
            let class_stays = match waiter.peek {
                true => {
                    if !record.persist {
                        self.records.remove(index);
                    }
                    record.persist
                }
                false => {
                    out.classes.insert(record.value);
                    let stays = self
                        .records
                        .iter()
                        .any(|member| member.value == record.value && member.persist);
                    self.records
                        .retain(|member| member.value != record.value || member.persist);
                    stays
                }
            };
            if waiter.peek && !record.persist {
                self.offer(record.value, false, record.hash.clone(), out);
            }
            let spent = !waiter.persist;
            assert!(
                spent || !(waiter.peek || class_stays),
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

/// One operation of a program of `mode`. Few values and few salts make classes of several
/// members. The receive names are filled in by [`arb_program`].
fn arb_op(mode: Mode) -> BoxedStrategy<SetOp> {
    let persistent = |weight_true: u32| prop_oneof![3 => Just(false), weight_true => Just(true)];
    let send =
        (0i64..4, 0u64..4, persistent(1)).prop_map(move |(value, salt, persistent)| SetOp::Send {
            value,
            salt,
            persistent: persistent && matches!(mode, Mode::PersistentSends),
        });
    let receive = (arb_pat(), 0u8..4)
        .prop_flat_map(|(pat, kind)| (Just(pat), arb_guard(pat), Just(kind)))
        .prop_map(move |(pat, guard, kind)| SetOp::Receive {
            name: 0,
            pat,
            guard,
            persistent: kind == 1 && matches!(mode, Mode::PersistentReceives),
            peek: kind == 2,
        });
    prop_oneof![3 => send, 2 => receive].boxed()
}

/// Names each receive of `ops` by its position.
fn named(ops: Vec<SetOp>) -> Vec<SetOp> {
    ops.into_iter()
        .enumerate()
        .map(|(index, op)| match op {
            SetOp::Receive {
                pat,
                guard,
                persistent,
                peek,
                ..
            } => SetOp::Receive {
                name: u64::try_from(index).expect("a small index"),
                pat,
                guard,
                persistent,
                peek,
            },
            send => send,
        })
        .collect()
}

/// A program of up to `max` operations. Every receive has a name of its own, and sends may
/// repeat a record (the salts are few), so a class can hold equal records.
fn arb_program(max: usize) -> BoxedStrategy<Vec<SetOp>> {
    prop_oneof![Just(Mode::PersistentSends), Just(Mode::PersistentReceives)]
        .prop_flat_map(move |mode| vec(arb_op(mode), 1..=max))
        .prop_map(named)
        .boxed()
}

/// A program that builds the scenarios of decision 37 and of Rule E. It stores records with
/// classes of several members. Guarded waiters meet a refused record first and park. Then come
/// more sends, peeks that remove a record and send it again, and receives that take whole
/// classes. A later send or a second send fires a parked waiter and retires its class.
fn arb_class_program() -> BoxedStrategy<Vec<SetOp>> {
    (
        vec((0i64..3, 0u64..4), 2..7),
        vec((0i64..3, proptest::bool::ANY), 1..4),
        vec(
            prop_oneof![2 => (0i64..3).prop_map(Pat::Lit), 1 => Just(Pat::Any)],
            0..3,
        ),
        vec((0i64..3, 0u64..3), 1..4),
        vec(
            prop_oneof![2 => (0i64..3).prop_map(Pat::Lit), 1 => Just(Pat::Any)],
            0..3,
        ),
    )
        .prop_map(|(records, waiters, peeks, tail, takes)| {
            let mut ops = Vec::with_capacity(
                records.len() + waiters.len() + peeks.len() + tail.len() + takes.len(),
            );
            for (value, salt) in records {
                ops.push(SetOp::Send {
                    value,
                    salt,
                    persistent: false,
                });
            }
            for (value, peek) in waiters {
                ops.push(SetOp::Receive {
                    name: 0,
                    pat: Pat::Any,
                    guard: Guard::Equals(value),
                    persistent: false,
                    peek,
                });
            }
            for (value, salt) in tail {
                ops.push(SetOp::Send {
                    value,
                    salt: salt + 10,
                    persistent: false,
                });
            }
            for pat in peeks {
                ops.push(SetOp::Receive {
                    name: 0,
                    pat,
                    guard: Guard::Free,
                    persistent: false,
                    peek: true,
                });
            }
            for pat in takes {
                ops.push(SetOp::Receive {
                    name: 0,
                    pat,
                    guard: Guard::Free,
                    persistent: false,
                    peek: false,
                });
            }
            named(ops)
        })
        .boxed()
}

/// A general program or a program of the class scenarios.
fn arb_any_program() -> BoxedStrategy<Vec<SetOp>> {
    prop_oneof![1 => arb_program(12), 2 => arb_class_program()].boxed()
}

// ─── The host differential ────────────────────────────────────────────────────────────────

/// The records that the host matcher may bind, or `None` for every record.
type Pin = Arc<Mutex<Option<Vec<ListParWithRandom>>>>;

/// The matcher of the host program: the Rholang matcher over the payload of a datum record
/// without its envelope, as the space agent matches (`spaces/matching.rs`). While a pin is set,
/// it binds only the pinned records. So a class fire and a K_retire bind the records that the
/// set semantics chooses. The guard of a parked continuation lies outside the envelope, so the
/// guard check needs no change.
#[derive(Clone)]
struct PinnedMatcher {
    pin: Pin,
}

impl Match<BindPattern, ListParWithRandom, TaggedContinuation> for PinnedMatcher {
    fn get(&self, pattern: &BindPattern, data: &ListParWithRandom) -> Option<ListParWithRandom> {
        let allowed = match self.pin.lock().expect("the pin lock").as_ref() {
            Some(pinned) => pinned.contains(data),
            None => true,
        };
        match (allowed, record::unwrap_datum(data)) {
            (false, _) => None,
            (true, Ok(record)) => Matcher.get(pattern, &record.datum),
            (true, Err(_)) => Matcher.get(pattern, data),
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

/// A `ChargingRSpace` over a fresh in-memory space with the [`PinnedMatcher`] of `pin`, which
/// charges `budget`.
async fn pinned_host(
    budget: &RuntimeBudget,
    pin: &Pin,
) -> impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation> {
    let mut stores = InMemoryStoreManager::new();
    let space = ParRSpace::create(
        stores.r_space_stores().await.expect("in-memory stores"),
        Arc::new(Box::new(PinnedMatcher { pin: pin.clone() })),
    )
    .expect("an in-memory space");
    let host: RhoISpace = Arc::new(Box::new(space));
    ChargingRSpace::charging_rspace(host, budget.clone())
}

/// Sets the pin to `records`, or clears it.
fn set_pin(pin: &Pin, records: Option<Vec<ListParWithRandom>>) {
    *pin.lock().expect("the pin lock") = records;
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

/// The fire of a host call as `fired` reports a fire of the agent: the body name and the values
/// that the first result binds. A class fire dispatches the result of its representative only,
/// and every member of a class binds the same values.
fn host_fire(continuation: &TaggedContinuation, results: &[ParResult]) -> (String, Vec<Par>) {
    let original = record::unwrap_continuation(continuation).expect("a continuation record");
    (
        body_name(&original.continuation),
        results
            .first()
            .map(|result| result.matched_datum.pars.clone())
            .unwrap_or_default(),
    )
}

/// The host program of RETIRE-CLASS after a fire of the element `element`. It makes one host
/// consume of the retire continuation over `[D]` for each stored non-persistent member of the
/// class, in canonical order. Each consume is pinned to its member.
async fn host_retire(
    host: &impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    budget: &RuntimeBudget,
    pin: &Pin,
    element: &ListParWithRandom,
) -> Result<(), TestCaseError> {
    let key = key_of_record(element);
    let random = RandomPurpose::Retire.derive(&Blake2b512Random::from_bytes(&element.random_state));
    let members: Vec<Datum<ListParWithRandom>> = canonical_records(host)
        .await
        .into_iter()
        .filter(|stored| !stored.persist && key_of_record(&stored.a) == key)
        .collect();
    for member in members {
        set_pin(pin, Some(vec![member.a.clone()]));
        let continuation =
            retire_continuation(&member.source.hash, &random, element.cost_authority.clone());
        let run = host_consume(
            host,
            budget,
            vec![data()],
            vec![retire_pattern()],
            continuation,
            false,
            false,
        )
        .await;
        set_pin(pin, None);
        prop_assert!(run.fired.is_some(), "the host retire binds its member");
    }
    Ok(())
}

/// What a receive does on the host records under the set semantics.
enum HostClass {
    /// The receive parks: no record binds its pattern, or its guard refuses the first one.
    Park,
    /// The receive fires over these records. The first is the first record in canonical order
    /// that the pattern binds and the guard admits. The other members of its class follow in
    /// canonical order, unless the receive peeks.
    Fire(Vec<ListParWithRandom>),
    /// No host consume binds these members. The host binds a persistent record again for each
    /// later occurrence of its channel, because a persistent record stays a candidate. So a
    /// host consume over `[D; n]` binds the members of the agent only in one case. At most one
    /// member is persistent, and it comes last in canonical order.
    Inexpressible,
}

/// The class fire of `receive` on the host records (see [`HostClass`]).
async fn host_class(
    host: &impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    receive: &ConsumeOp,
) -> HostClass {
    let records = canonical_records(host).await;
    let Some((first, matched)) = records.iter().find_map(|stored| {
        let payload = record::unwrap_datum(&stored.a)
            .expect("a datum record")
            .datum;
        Matcher
            .get(&receive.pattern, &payload)
            .map(|matched| (stored.clone(), matched))
    }) else {
        return HostClass::Park;
    };
    if !Matcher.check_commit(&parked(receive), std::slice::from_ref(&matched)) {
        return HostClass::Park;
    }
    if receive.peek {
        return HostClass::Fire(vec![first.a]);
    }
    let key = key_of_record(&first.a);
    let members: Vec<Datum<ListParWithRandom>> = records
        .into_iter()
        .filter(|stored| key_of_record(&stored.a) == key)
        .collect();
    let persistent: Vec<usize> = members
        .iter()
        .enumerate()
        .filter(|(_, member)| member.persist)
        .map(|(index, _)| index)
        .collect();
    match persistent.as_slice() {
        [] => HostClass::Fire(members.into_iter().map(|member| member.a).collect()),
        [index] if *index + 1 == members.len() => {
            HostClass::Fire(members.into_iter().map(|member| member.a).collect())
        }
        _ => HostClass::Inexpressible,
    }
}

/// Runs the host program of `op` on `host`: the host call of each staged operation of the set
/// semantics, then the calls of the reducer after each fire. A class fire is a consume over
/// `[D; n]`. The K_retire consumes of its class follow a fire of a waiter that does not peek. A
/// persistent send produces again, and a persistent receive consumes again. A fire of a peek
/// sends each removed non-persistent record again (`produce_peeks`). The program returns the
/// fires in order, or `None` when a class fire has no host consume ([`HostClass`]).
async fn host_program(
    host: &impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    budget: &RuntimeBudget,
    pin: &Pin,
    op: &SpaceOp,
) -> Result<Option<Vec<(String, Vec<Par>)>>, TestCaseError> {
    let mut calls = VecDeque::from([match op {
        SpaceOp::Produce(send) => HostCall::Send {
            record: record::wrap_datum(0, &send.data),
            persist: send.persistent,
        },
        SpaceOp::Consume(receive) => HostCall::Receive(Box::new(receive.clone())),
        SpaceOp::Settle(_) => panic!("a set issues no settle"),
        SpaceOp::Join(_) => panic!("the set programs of this suite issue no join"),
    }]);
    let mut fires = Vec::new();
    while let Some(call) = calls.pop_front() {
        let (again, reproduced) = match call {
            HostCall::Send { record, persist } => {
                set_pin(pin, None);
                let run = host_produce(host, budget, data(), record.clone(), persist).await;
                let mut again = None;
                if let Some((continuation, results)) = &run.fired {
                    fires.push(host_fire(&continuation.continuation, results));
                    if !continuation.peek {
                        host_retire(host, budget, pin, &record).await?;
                    }
                    if persist {
                        again = Some(HostCall::Send { record, persist });
                    }
                }
                (again, run.reproduced)
            }
            HostCall::Receive(receive) => match host_class(host, &receive).await {
                HostClass::Inexpressible => return Ok(None),
                HostClass::Park => {
                    set_pin(pin, None);
                    let run = host_consume(
                        host,
                        budget,
                        vec![data()],
                        vec![receive.pattern.clone()],
                        parked(&receive),
                        receive.persistent,
                        receive.peek,
                    )
                    .await;
                    prop_assert!(run.fired.is_none(), "the host receive parks");
                    (None, Vec::new())
                }
                HostClass::Fire(class) => {
                    let width = class.len();
                    set_pin(pin, Some(class));
                    let run = host_consume(
                        host,
                        budget,
                        vec![data(); width],
                        vec![receive.pattern.clone(); width],
                        parked(&receive),
                        receive.persistent,
                        receive.peek,
                    )
                    .await;
                    set_pin(pin, None);
                    let Some((continuation, results)) = &run.fired else {
                        return Err(TestCaseError::fail("the host class fire binds its class"));
                    };
                    fires.push(host_fire(&continuation.continuation, results));
                    let again = match receive.persistent {
                        true => Some(HostCall::Receive(receive)),
                        false => None,
                    };
                    (again, run.reproduced)
                }
            },
        };
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
    Ok(Some(fires))
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

/// Compares the agent and the host program of the operations `ops`, operation by operation:
/// the new legacy log entries (value and label), the fires, and at the end the stores. Each side
/// runs inside the legacy scope of its own budget. It returns `false` when it stops at a class
/// fire that no host consume makes ([`HostClass::Inexpressible`]). It returns `true` when it
/// compares every operation.
async fn compare_with_host(ops: &[SpaceOp]) -> Result<bool, TestCaseError> {
    let agent_budget = RuntimeBudget::new(Cost::create(LEGACY_PHLO, "rrs set agent"));
    let _agent_scope = agent_budget.enter_legacy_scope();
    let rig = Rig::with_budget(&agent_budget).await;
    let host_budget = RuntimeBudget::new(Cost::create(LEGACY_PHLO, "rrs set host"));
    let _host_scope = host_budget.enter_legacy_scope();
    let pin: Pin = Arc::new(Mutex::new(None));
    let host = pinned_host(&host_budget, &pin).await;
    for (index, op) in ops.iter().enumerate() {
        let agent_before = agent_budget.get_log().len();
        let outcome = rig
            .run(op.clone())
            .await
            .map_err(|error| TestCaseError::fail(format!("operation {index} fails: {error}")))?;
        let agent_entries = agent_budget.get_log()[agent_before..].to_vec();
        let host_before = host_budget.get_log().len();
        let Some(host_fires) = host_program(&host, &host_budget, &pin, op).await? else {
            return Ok(false);
        };
        let host_entries = host_budget.get_log()[host_before..].to_vec();
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
    Ok(true)
}

// ─── Example tests ────────────────────────────────────────────────────────────────────────

/// The store keeps every record (F5). Two sends of one value with two random states and two
/// byte-identical sends store four records, each with the `ord` 0. The set has no counter. The
/// view removes the duplicates, and the store does not.
#[tokio::test]
async fn space_set_discipline_duplicates_stay_distinct_records() {
    let rig = Rig::new().await;
    let twin = salted(5, 1, false);
    rig.send(twin.clone()).await;
    rig.send(twin.clone()).await;
    rig.send(salted(5, 2, false)).await;
    rig.send(salted(6, 1, false)).await;
    let log = rig.log().await;
    assert_eq!(labels(&log), vec![
        "produce D",
        "produce D",
        "produce D",
        "produce D"
    ]);
    let stored = produces(&log);
    assert_eq!(stored[0], stored[1], "two equal sends have one identity");
    assert_ne!(
        stored[0], stored[2],
        "another random state, another identity"
    );
    assert_eq!(rig.values().await, vec![
        (5, false),
        (5, false),
        (5, false),
        (6, false)
    ]);
    assert!(counter(&rig.host, &counter_channel(SPACE, &channel()))
        .await
        .is_empty());
    assert_eq!(
        counter(&rig.host, &space_counter_channel(SPACE)).await,
        vec![0]
    );
    assert!(
        labels(&log).iter().all(|label| label.ends_with(" D")),
        "no event touches a counter"
    );
}

/// The view of a set holds one representative for each class (F7, decision 41). Receives of any
/// value drain the set class by class. Each fire dispatches the first member of its class in
/// canonical order, and a third receive parks.
#[tokio::test]
async fn space_set_discipline_view_dedups_equal_pars() {
    let rig = Rig::new().await;
    for (value, salt) in [(1, 1), (1, 2), (2, 1), (2, 2), (2, 3)] {
        rig.send(salted(value, salt, false)).await;
    }
    let mut drained = Vec::new();
    for name in ["a", "b"] {
        let first = canonical_records(&rig.host).await[0].a.clone();
        let outcome = rig.receive(receiving(name, any())).await;
        assert_eq!(outcome.fires.len(), 1);
        assert_eq!(
            removed(&outcome, 0),
            first,
            "the representative is the first record in canonical order"
        );
        let value = record_value(&first);
        assert!(
            rig.values()
                .await
                .iter()
                .all(|(stored, _)| *stored != value),
            "the class of {value} leaves the set"
        );
        drained.push(value);
    }
    drained.sort();
    assert_eq!(drained, vec![1, 2], "one fire for each class");
    assert!(rig.receive(receiving("c", any())).await.fires.is_empty());
    assert_eq!(rig.waiters().await, vec!["c".to_string()]);
}

/// A receive that does not peek fires one consume over `[D; n]` with every equal record, and the
/// outcome dispatches one result (F3, F8, F20).
#[tokio::test]
async fn space_set_discipline_consume_retires_all_equal_records() {
    let rig = Rig::new().await;
    for salt in 1..=3 {
        rig.send(salted(4, salt, false)).await;
    }
    rig.send(salted(5, 1, false)).await;
    rig.log().await;
    let outcome = rig.receive(receiving("k", exactly(4))).await;
    assert_eq!(fired(&outcome), vec![delivery("k", &[])]);
    assert_eq!(outcome.fires[0].results.len(), 1, "one dispatched result");
    assert_eq!(labels(&rig.log().await), vec![
        "consume D,D,D",
        "comm D,D,D"
    ]);
    assert_eq!(rig.values().await, vec![(5, false)]);
}

/// The COMM of a class fire lists the produce of every member, the representative first (F9).
/// Its consume lists the data channel once for each member, with no peek.
#[tokio::test]
async fn space_set_discipline_retire_comm_lists_every_member() {
    let rig = Rig::new().await;
    let members = ordered_sends(&[(3, false), (3, false), (3, false)]);
    for member in &members {
        rig.send(member.clone()).await;
    }
    rig.send(salted(9, 1, false)).await;
    rig.log().await;
    let receive = receiving("k", exactly(3));
    rig.receive(receive.clone()).await;
    let log = rig.log().await;
    let [comm] = comms(&log).try_into().expect("one COMM");
    let listed: Vec<Blake2b256Hash> = comm
        .produces
        .iter()
        .map(|produce| produce.hash.clone())
        .collect();
    let expected: Vec<Blake2b256Hash> = members.iter().map(record_hash).collect();
    assert_eq!(listed, expected, "every member, in canonical order");
    assert!(comm.peeks.is_empty());
    let data_hash = stable_hash_provider::hash(&data());
    assert_eq!(comm.consume.channel_hashes, vec![data_hash; 3]);
    assert_eq!(
        comm.consume,
        Consume::create(
            &vec![data(); 3],
            &vec![receive.pattern.clone(); 3],
            &parked(&receive),
            false
        ),
        "the consume of the class fire over [D; 3]"
    );
    assert_eq!(rig.values().await, vec![(9, false)]);
}

/// A peek binds the representative alone. Its fire removes the record, as the host COMM does,
/// and the same operation sends the record again with its identity and its own authority. No
/// K_retire runs, and the other members of the class stay (decision 45, Rule E).
#[tokio::test]
async fn space_set_discipline_peek_restores_representative() {
    let budget = budget();
    let _scope = budget.enter_comm_accounting_scope();
    let rig = Rig::with_budget(&budget).await;
    let mut representative = salted(7, 1, false);
    representative.authority = region(2);
    representative.data.cost_authority = Some(region(2));
    let other = (2..)
        .map(|salt| salted(7, salt, false))
        .find(|other| record_hash(&representative) < record_hash(other))
        .expect("a salt that orders the other member after the representative");
    let members = [representative, other];
    for member in &members {
        rig.send(member.clone()).await;
    }
    rig.log().await;
    let peek = ConsumeOp {
        introduction_authority: region(4),
        ..peeking(receiving("p", any()))
    };
    let outcome = rig.receive(peek).await;
    assert_eq!(fired(&outcome), vec![delivery("p", &[7])]);
    assert_eq!(
        removed(&outcome, 0),
        record::wrap_datum(0, &members[0].data),
        "the representative"
    );
    assert_eq!(outcome.reissue, None);
    let log = rig.log().await;
    assert_eq!(labels(&log), vec!["consume D", "comm D peek", "produce D"]);
    assert_eq!(
        produces(&log)[0].hash,
        record_hash(&members[0]),
        "the second send has the identity of the representative"
    );
    assert_eq!(
        registered(&budget, &log[2]),
        Some(region(2)),
        "the second send registers the authority of the record"
    );
    assert_eq!(registered(&budget, &log[0]), Some(region(4)));
    assert_eq!(rig.values().await, vec![(7, false), (7, false)]);
}

/// The second send of a peeked representative is offered to the waiters first. When it fires a
/// guarded waiter that does not peek, that fire retires the rest of the class with K_retire
/// (decision 37, decision 45).
#[tokio::test]
async fn space_set_discipline_peek_restore_reoffer_retires_class() {
    let rig = Rig::new().await;
    let sends = ordered_sends(&[(1, false), (2, false), (2, false)]);
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
        "comm D",
        "consume D",
        "comm D"
    ]);
    assert_eq!(
        consumes(&log)[1],
        retire_consume(
            &record::wrap_datum(0, &sends[1].data),
            &record_hash(&sends[2])
        ),
        "the second send of the first 2 retires the second 2"
    );
    assert_eq!(rig.values().await, vec![(1, false)]);
    assert!(rig.waiters().await.is_empty());
}

/// The members of a class bind in canonical order, whatever the insertion order (decision 41).
/// The K_retire COMMs of a class follow the canonical order too.
#[tokio::test]
async fn space_set_discipline_members_in_canonical_order() {
    let rig = Rig::new().await;
    let members = ordered_sends(&[(3, false), (3, false), (3, false)]);
    for member in members.iter().rev() {
        rig.send(member.clone()).await;
    }
    rig.log().await;
    let outcome = rig.receive(receiving("k", exactly(3))).await;
    assert_eq!(
        removed(&outcome, 0),
        record::wrap_datum(0, &members[0].data)
    );
    let [comm] = comms(&rig.log().await).try_into().expect("one COMM");
    let listed: Vec<Blake2b256Hash> = comm
        .produces
        .iter()
        .map(|produce| produce.hash.clone())
        .collect();
    assert_eq!(listed, members.iter().map(record_hash).collect::<Vec<_>>());

    let rig = Rig::new().await;
    let stored = ordered_sends(&[(1, false), (3, false), (3, false), (3, false)]);
    for send in stored.iter().rev() {
        rig.send(send.clone()).await;
    }
    rig.receive(guarded(receiving("w", any()), equals(3))).await;
    rig.log().await;
    let incoming = salted(3, 1_000, false);
    rig.send(incoming.clone()).await;
    let log = rig.log().await;
    let retired: Vec<Consume> = consumes(&log);
    let element = record::wrap_datum(0, &incoming.data);
    let expected: Vec<Consume> = stored[1..]
        .iter()
        .map(|member| retire_consume(&element, &record_hash(member)))
        .collect();
    assert_eq!(
        retired, expected,
        "one K_retire for each member, in canonical order"
    );
    assert_eq!(rig.values().await, vec![(1, false)]);
}

/// A class with a persistent member stays in the set. The class fire binds the persistent member
/// with the others and keeps it, and a later receive finds the class again.
#[tokio::test]
async fn space_set_discipline_persistent_member_keeps_class() {
    let rig = Rig::new().await;
    for send in ordered_sends(&[(6, true), (6, false)]) {
        rig.send(send).await;
    }
    rig.log().await;
    let outcome = rig.receive(receiving("a", exactly(6))).await;
    assert_eq!(fired(&outcome), vec![delivery("a", &[])]);
    assert_eq!(labels(&rig.log().await), vec!["consume D,D", "comm D,D"]);
    assert_eq!(rig.values().await, vec![(6, true)]);
    let outcome = rig.receive(receiving("b", exactly(6))).await;
    assert_eq!(fired(&outcome), vec![delivery("b", &[])]);
    assert!(outcome.fires[0].results[0].persistent);
    assert_eq!(rig.values().await, vec![(6, true)], "the class stays");
}

/// A class fire touches its own class only: the other classes keep every member.
#[tokio::test]
async fn space_set_discipline_distinct_classes_unaffected() {
    let rig = Rig::new().await;
    for (value, salt) in [(1, 1), (1, 2), (2, 1), (2, 2), (3, 1), (3, 2), (3, 3)] {
        rig.send(salted(value, salt, false)).await;
    }
    rig.receive(receiving("k", exactly(2))).await;
    assert_eq!(rig.values().await, vec![
        (1, false),
        (1, false),
        (3, false),
        (3, false),
        (3, false)
    ]);
}

/// The class key depends on the payload only (F5). Two records of one value with other random
/// states and other cost authorities are one class, and a receive takes both. The cost stack
/// lies outside the payload too, so it does not change the key. The host matcher binds no datum
/// that carries a cost stack. Only a funding-channel datum of the cost-accounting branch carries
/// one, and no space send does. So a class fire whose member carries a cost stack fails closed
/// with `Inconsistent` and stages nothing.
#[tokio::test]
async fn space_set_discipline_key_ignores_random_state_and_authority() {
    let rig = Rig::new().await;
    let mut first = salted(9, 1, false);
    first.authority = region(1);
    first.data.cost_authority = Some(region(1));
    let mut second = salted(9, 2, false);
    second.authority = region(2);
    second.data.cost_authority = Some(region(2));
    assert_ne!(first.data, second.data, "two different records");
    assert_eq!(
        set_key(&first.data.pars, &ScanMeter::new(None)),
        set_key(&second.data.pars, &ScanMeter::new(None))
    );
    rig.send(first).await;
    rig.send(second).await;
    rig.log().await;
    rig.receive(receiving("k", exactly(9))).await;
    assert_eq!(labels(&rig.log().await), vec!["consume D,D", "comm D,D"]);
    assert!(rig.values().await.is_empty(), "one class leaves at once");

    let rig = Rig::new().await;
    let plain = salted(4, 1, false);
    let stacked = (2..)
        .map(|salt| {
            let mut stacked = salted(4, salt, false);
            stacked.data.cost_stack = Some(CostStack {
                cells: vec![CostSignature {
                    value: Some(CostSignatureValue::Ground(vec![7])),
                }],
            });
            stacked
        })
        .find(|stacked| record_hash(&plain) < record_hash(stacked))
        .expect("a salt that orders the stacked record after the plain one");
    assert_eq!(
        record::unwrap_datum(&record::wrap_datum(0, &stacked.data))
            .expect("a record")
            .datum
            .pars,
        plain.data.pars,
        "the cost stack lies outside the payload, so the key is the key of 4"
    );
    rig.send(plain).await;
    rig.send(stacked).await;
    rig.log().await;
    let before = store_bytes(&rig.host).await;
    assert_eq!(
        rig.run(SpaceOp::Consume(receiving("k", exactly(4)))).await,
        Err(SpaceError::Inconsistent(
            "a member of a class does not bind the pattern of its representative"
        )),
        "the member with the cost stack does not bind, so the class fire fails closed"
    );
    assert!(rig.log().await.is_empty(), "nothing is staged");
    assert_eq!(store_bytes(&rig.host).await, before);
}

/// The key frames the count of the elements and the length of each element (F5, F6). So
/// payloads that differ in arity, in the order of their elements or in their nesting are other
/// classes.
#[tokio::test]
async fn space_set_discipline_key_length_prefix_no_collision() {
    let meter = ScanMeter::new(None);
    let pair = vec![int(1), int(2)];
    let key = set_key(&pair, &meter).expect("a key");
    let mut expected = 2u64.to_be_bytes().to_vec();
    for element in &pair {
        let bytes = bincode::serialize(&ParSortMatcher::sort_match(element).term)
            .expect("a Par serializes");
        expected.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
        expected.extend_from_slice(&bytes);
    }
    assert_eq!(key, expected, "the count, then each length and its bytes");
    assert_eq!(&key[..LENGTH_BYTES], &2u64.to_be_bytes());
    let nested = Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                ps: pair.clone(),
                ..EList::default()
            })),
        }],
        ..Par::default()
    };
    let payloads = [
        pair.clone(),
        vec![int(2), int(1)],
        vec![int(1)],
        vec![int(2)],
        vec![nested.clone()],
    ];
    let keys: BTreeSet<Vec<u8>> = payloads
        .iter()
        .map(|payload| set_key(payload, &meter).expect("a key"))
        .collect();
    assert_eq!(
        keys.len(),
        payloads.len(),
        "every payload is a class of its own"
    );

    let rig = Rig::new().await;
    rig.send(send_pars(pair.clone(), 1)).await;
    rig.send(send_pars(pair.clone(), 2)).await;
    for (salt, payload) in payloads.iter().enumerate().skip(1) {
        rig.send(send_pars(payload.clone(), 10 + salt as u64)).await;
    }
    rig.log().await;
    rig.receive(receiving("k", literal(pair.clone()))).await;
    assert_eq!(labels(&rig.log().await), vec!["consume D,D", "comm D,D"]);
    let mut left: Vec<Vec<Par>> = rig
        .records()
        .await
        .iter()
        .map(|stored| {
            record::unwrap_datum(&stored.a)
                .expect("a record")
                .datum
                .pars
        })
        .collect();
    left.sort_by_key(|pars| format!("{pars:?}"));
    let mut expected_left: Vec<Vec<Par>> = payloads[1..].to_vec();
    expected_left.sort_by_key(|pars| format!("{pars:?}"));
    assert_eq!(left, expected_left, "only the class of (1, 2) leaves");
}

/// A term deeper than the depth limit of the metered sorter has no key under a host-work
/// budget (decision 42, v3 C.1 item 7). A receive that must compute that key fails with
/// `HostWorkRejected` before it stages anything. A send whose fire must retire its class fails
/// in the same way, and so does a metered send that would store the record: a metered job
/// stores no record without a key. Nothing happens: no event, no record change and no legacy
/// charge. This holds in play, in a rigged replay and after a checkpoint reset. Without a budget
/// the key exists, as the host sorts any depth, so an unmetered job stores the record.
#[test]
fn space_set_discipline_failed_canonicalization_rejects_before_staging() {
    std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(|| tokio_runtime().block_on(failed_canonicalization()))
        .expect("a test thread starts")
        .join()
        .expect("the canonicalization test passes");
}

/// A list nested `depth` levels deep around the integer 0.
fn nested_list(depth: usize) -> Par {
    let mut par = int(0);
    for _ in 0..depth {
        par = Par {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::EListBody(EList {
                    ps: vec![par],
                    ..EList::default()
                })),
            }],
            ..Par::default()
        };
    }
    par
}

/// The body of [`space_set_discipline_failed_canonicalization_rejects_before_staging`].
async fn failed_canonicalization() {
    let deep = send_pars(vec![nested_list(260)], 1);
    let legacy = RuntimeBudget::new(Cost::create(LEGACY_PHLO, "rrs set canonicalization"));
    let _scope = legacy.enter_legacy_scope();
    let (play, replay) = play_and_replay().await;
    let rig = Rig::over(play.clone(), &legacy).await;
    rig.send(deep.clone()).await;
    let start = play
        .create_checkpoint()
        .await
        .expect("a checkpoint with the deep record")
        .root;
    let metered = Rig {
        host: rig.host.clone(),
        env: rig.env.clone(),
    }
    .metered(&unbounded_host_work());
    let expect_rejected = |result: Result<SpaceOutcome, SpaceError>, what: &str| {
        assert_eq!(
            result,
            Err(SpaceError::Host(RSpaceError::HostWorkRejected)),
            "{what}: the key of the deep payload is rejected"
        );
    };
    let charges = legacy.get_log().len();
    let before = store_bytes(&play).await;
    expect_rejected(
        metered.run(SpaceOp::Consume(receiving("k", any()))).await,
        "the class receive",
    );
    assert!(play.take_event_log().await.is_empty(), "nothing is staged");
    assert_eq!(store_bytes(&play).await, before, "no record changes");
    assert_eq!(legacy.get_log().len(), charges, "no observer call charges");
    // A send of 5 fires the waiter for 5, which does not peek, so RETIRE-CLASS reads the key of
    // every stored member candidate. The key of the deep record fails there, before the commit,
    // so the dropped transaction applies nothing: under a budget, a stored deep record blocks
    // every retire of the collection.
    rig.receive(receiving("w", exactly(5))).await;
    play.take_event_log().await;
    let parked_before = store_bytes(&play).await;
    expect_rejected(
        metered
            .run(SpaceOp::Produce(send_pars(vec![int(5)], 2)))
            .await,
        "the retire of a send of 5",
    );
    assert!(play.take_event_log().await.is_empty(), "nothing is staged");
    assert_eq!(store_bytes(&play).await, parked_before);
    // Without a budget the host sorts any depth, so the receive takes the class.
    let unmetered = rig.receive(receiving("u", any())).await;
    assert_eq!(unmetered.fires.len(), 1);
    // The send of the deep payload fires a waiter that does not peek, so it must compute the
    // key of its own class first: the key fails before the fire stages. The waiter parks on
    // the empty set first.
    let empty = Rig::with_budget(&legacy).await;
    empty
        .receive(ConsumeOp {
            pattern: literal(deep.data.pars.clone()),
            ..receiving("d", any())
        })
        .await;
    assert_eq!(empty.waiters().await, vec!["d".to_string()]);
    empty.log().await;
    let parked_before = store_bytes(&empty.host).await;
    let metered_empty = Rig {
        host: empty.host.clone(),
        env: empty.env.clone(),
    }
    .metered(&unbounded_host_work());
    expect_rejected(
        metered_empty
            .run(SpaceOp::Produce(send_pars(deep.data.pars.clone(), 3)))
            .await,
        "the send of the deep payload",
    );
    assert!(empty.log().await.is_empty(), "nothing is staged");
    assert_eq!(store_bytes(&empty.host).await, parked_before);
    // A metered send of the deep payload into a set without a waiter fails at the store, so a
    // metered job never stores a record that would block the later class operations.
    let fresh = Rig::with_budget(&legacy).await;
    fresh.log().await;
    let fresh_before = store_bytes(&fresh.host).await;
    let metered_fresh = Rig {
        host: fresh.host.clone(),
        env: fresh.env.clone(),
    }
    .metered(&unbounded_host_work());
    expect_rejected(
        metered_fresh
            .run(SpaceOp::Produce(send_pars(deep.data.pars.clone(), 4)))
            .await,
        "the store of the deep payload",
    );
    assert!(fresh.log().await.is_empty(), "nothing is staged");
    assert_eq!(
        store_bytes(&fresh.host).await,
        fresh_before,
        "no record is stored"
    );
    // Negative control: a bag has no class key, so the same metered receive of the deep record
    // fires. The failure of the set is the failure of its key, not of the matcher.
    let bag_host = support::agent::play().await;
    let bag_env = env_over(bag_host.clone(), &budget());
    let bag_config = config_of(SPACE, "hashmap", "bag", SpaceQualifier::Default);
    let bag_family = SpaceFactoryUrn {
        outer: SpaceOuter::HashMap,
        inner: SpaceInner::Bag,
    };
    create(
        &Bag::default(),
        &bag_env,
        creation(SPACE, bag_config, bag_family),
    )
    .await;
    run(&Bag::default(), &bag_env, SpaceOp::Produce(deep.clone()))
        .await
        .expect("the bag stores the deep record");
    let metered_bag = SpaceEnv {
        meter: ScanMeter::new(Some(unbounded_host_work())),
        ..bag_env
    };
    let bag_outcome = run(
        &Bag::default(),
        &metered_bag,
        SpaceOp::Consume(receiving("k", any())),
    )
    .await
    .expect("the metered bag receive fires");
    assert_eq!(bag_outcome.fires.len(), 1);
    // A rigged replay and a reset fail at the same point with the same effect.
    let Checkpoint { log, .. } = play.create_checkpoint().await.expect("a checkpoint");
    replay
        .rig_and_reset(start.clone(), log)
        .await
        .expect("the replay rigs the log");
    let replay_rig = Rig {
        host: play.clone(),
        env: SpaceEnv {
            meter: ScanMeter::new(Some(unbounded_host_work())),
            ..env_over(replay.clone(), &legacy)
        },
    };
    let replay_before = store_bytes(&replay).await;
    expect_rejected(
        replay_rig
            .run(SpaceOp::Consume(receiving("k", any())))
            .await,
        "the replayed class receive",
    );
    assert_eq!(store_bytes(&replay).await, replay_before);
    play.reset(&start).await.expect("the reset to the start");
    let reset_before = store_bytes(&play).await;
    expect_rejected(
        metered.run(SpaceOp::Consume(receiving("k", any()))).await,
        "the class receive after the reset",
    );
    assert!(play.take_event_log().await.is_empty());
    assert_eq!(store_bytes(&play).await, reset_before);
}

/// The case of readiness review S3 (decision 37). A guarded waiter parks while a stored member
/// passes its guard, because the guard is checked once on its first spatial match. A later send
/// of an equal member fires the waiter with the incoming member. K_retire then retires the stored
/// member, so the class leaves the set. Replay stages the same events.
#[tokio::test]
async fn space_set_discipline_guarded_waiter_then_equal_send_retires_class() {
    let (play, replay) = play_and_replay().await;
    let budget = budget();
    let rig = Rig::over(play.clone(), &budget).await;
    let start = play.create_checkpoint().await.expect("a checkpoint").root;
    let stored = ordered_sends(&[(1, false), (2, false)]);
    let incoming = salted(2, 1_000, false);
    let script = vec![
        SpaceOp::Produce(stored[0].clone()),
        SpaceOp::Produce(stored[1].clone()),
        SpaceOp::Consume(guarded(receiving("w", any()), equals(2))),
        SpaceOp::Produce(incoming.clone()),
    ];
    assert!(
        record_hash(&stored[0]) < record_hash(&stored[1]),
        "the class {{1}} precedes the class {{2}}"
    );
    let mut outcomes = Vec::with_capacity(script.len());
    for op in &script {
        outcomes.push(rig.run(op.clone()).await.expect("the script plays"));
    }
    assert!(
        outcomes[2].fires.is_empty(),
        "the guard refuses its first match 1"
    );
    assert_eq!(fired(&outcomes[3]), vec![delivery("w", &[2])]);
    assert_eq!(rig.values().await, vec![(1, false)]);
    assert!(rig.waiters().await.is_empty());
    let Checkpoint { root, log } = play.create_checkpoint().await.expect("a checkpoint");
    let last = &log[log.len() - 4..];
    assert_eq!(labels(last), vec![
        "produce D",
        "comm D",
        "consume D",
        "comm D"
    ]);
    let retire = retire_consume(
        &record::wrap_datum(0, &incoming.data),
        &record_hash(&stored[1]),
    );
    assert_eq!(last[2], Event::IoEvent(IOEvent::Consume(retire.clone())));
    let Event::Comm(comm) = &last[3] else {
        panic!("the retire COMM")
    };
    assert_eq!(comm.consume, retire);
    assert_eq!(
        comm.produces
            .iter()
            .map(|produce| produce.hash.clone())
            .collect::<Vec<_>>(),
        vec![record_hash(&stored[1])],
        "the retire binds the stored 2"
    );
    replay
        .rig_and_reset(start, log)
        .await
        .expect("the replay rigs the play log");
    let replay_env = env_over(replay.clone(), &budget);
    let mut replayed = Vec::with_capacity(script.len());
    for op in script {
        replayed.push(
            run(&Set::default(), &replay_env, op)
                .await
                .expect("the script replays"),
        );
    }
    assert_eq!(replayed, outcomes);
    replay
        .check_replay_data()
        .await
        .expect("every recorded COMM is paired");
    assert_eq!(
        replay.create_checkpoint().await.expect("a checkpoint").root,
        root
    );
}

/// The shape of RETIRE-CLASS: one K_retire consume over `[D]` for each stored non-persistent
/// member, in canonical order. Each retire has the retire pattern and the source hash of its
/// member as its body. It has the random state of the element under `RandomPurpose::Retire`, the
/// cost authority of the element and no peek. Its COMM binds its member. A persistent member
/// stays, and the retires are not dispatched.
#[tokio::test]
async fn space_set_discipline_produce_fire_retire_events_shape() {
    let rig = Rig::new().await;
    let stored = ordered_sends(&[(1, false), (2, false), (2, false), (2, true)]);
    for send in &stored {
        rig.send(send.clone()).await;
    }
    rig.receive(guarded(receiving("w", any()), equals(2))).await;
    assert_eq!(rig.waiters().await, vec!["w".to_string()]);
    rig.log().await;
    let mut incoming = salted(2, 1_000, false);
    incoming.authority = region(9);
    incoming.data.cost_authority = Some(region(9));
    let outcome = rig.send(incoming.clone()).await;
    assert_eq!(outcome.fires.len(), 1, "the retires are not dispatched");
    let log = rig.log().await;
    assert_eq!(labels(&log), vec![
        "produce D",
        "comm D",
        "consume D",
        "comm D",
        "consume D",
        "comm D"
    ]);
    let element = record::wrap_datum(0, &incoming.data);
    let expected: Vec<Consume> = stored[1..3]
        .iter()
        .map(|member| retire_consume(&element, &record_hash(member)))
        .collect();
    assert_eq!(consumes(&log), expected);
    let retires: Vec<COMM> = comms(&log)[1..].to_vec();
    for (comm, member) in retires.iter().zip(&stored[1..3]) {
        assert!(comm.peeks.is_empty());
        assert!(!comm.consume.persistent);
        assert_eq!(
            comm.produces
                .iter()
                .map(|produce| produce.hash.clone())
                .collect::<Vec<_>>(),
            vec![record_hash(member)]
        );
    }
    let continuation = retire_continuation(
        &record_hash(&stored[1]),
        &RandomPurpose::Retire.derive(&Blake2b512Random::from_bytes(&incoming.data.random_state)),
        Some(region(9)),
    );
    assert_eq!(continuation.cost_authority, Some(region(9)));
    assert_eq!(continuation.guard, None);
    assert_eq!(rig.values().await, vec![(1, false), (2, true)]);
}

/// Waiters are equal only when they are byte-identical (F14). Two receives with one pattern and
/// two random states park two waiters. A byte-identical receive parks nothing new. A send fires
/// one of them, the first in canonical order.
#[tokio::test]
async fn space_set_discipline_equal_receives_stay_two_waiters() {
    let rig = Rig::new().await;
    let receives = ordered_receives(vec![receiving("w", any()), receiving("w", any())]);
    assert_eq!(receives[0].pattern, receives[1].pattern);
    assert_ne!(waiter_hash(&receives[0]), waiter_hash(&receives[1]));
    rig.receive(receives[1].clone()).await;
    rig.receive(receives[0].clone()).await;
    assert_eq!(rig.waiters().await, vec!["w".to_string(), "w".to_string()]);
    rig.receive(receives[0].clone()).await;
    assert_eq!(
        rig.waiters().await,
        vec!["w".to_string(), "w".to_string()],
        "a byte-identical park adds nothing"
    );
    rig.log().await;
    rig.send(salted(1, 1, false)).await;
    let log = rig.log().await;
    let [comm] = comms(&log).try_into().expect("one COMM");
    assert_eq!(
        comm.consume.hash,
        waiter_hash(&receives[0]),
        "the first waiter in canonical order fires"
    );
    assert_eq!(rig.waiters().await, vec!["w".to_string()]);
}

/// After a checkpoint and a reset, the store holds the records in the order of the history. The
/// agent makes the same class fire and reaches the same root.
#[tokio::test]
async fn space_set_discipline_checkpoint_restart() {
    let rig = Rig::new().await;
    for (value, salt) in [(1, 1), (2, 1), (1, 2), (3, 1), (1, 3)] {
        rig.send(salted(value, salt, false)).await;
    }
    let start = rig
        .host
        .create_checkpoint()
        .await
        .expect("a checkpoint")
        .root;
    let first = rig.receive(receiving("a", any())).await;
    let first_removed = removed(&first, 0);
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
    assert_eq!(rig.values().await.len(), 5);
    let again = rig.receive(receiving("a", any())).await;
    assert_eq!(fired(&again), fired(&first));
    assert_eq!(
        removed(&again, 0),
        first_removed,
        "the choice does not depend on the list order"
    );
    assert_eq!(
        rig.host
            .create_checkpoint()
            .await
            .expect("a checkpoint")
            .root,
        after
    );
}

/// The script of the replay test. It stores equal records and parks a guarded waiter. A peek
/// follows, whose second send fires the waiter and retires the class. A class fire takes two
/// byte-identical records, and a peek stores its record again. Then come parks, a persistent send
/// that fires and is stored, and a persistent receive that drains a class and parks. A divergent
/// pair ends the script. The records follow each other in canonical order, so every operation
/// makes a known number of fires ([`REPLAY_FIRES`]).
fn replay_script() -> Vec<SpaceOp> {
    let sends = ordered_sends(&[(1, false), (2, false), (2, false), (5, false)]);
    vec![
        SpaceOp::Produce(sends[0].clone()),
        SpaceOp::Produce(sends[1].clone()),
        SpaceOp::Produce(sends[2].clone()),
        SpaceOp::Produce(sends[3].clone()),
        SpaceOp::Produce(sends[3].clone()),
        SpaceOp::Consume(guarded(receiving("g", any()), equals(2))),
        SpaceOp::Consume(peeking(receiving("p", exactly(2)))),
        SpaceOp::Consume(receiving("a", exactly(5))),
        SpaceOp::Consume(peeking(receiving("q", any()))),
        SpaceOp::Consume(receiving("t", exactly(7))),
        SpaceOp::Consume(guarded(receiving("refuses", any()), refusing())),
        SpaceOp::Produce(salted(7, 1, true)),
        SpaceOp::Consume(persisting(receiving("drain", exactly(1)))),
        SpaceOp::Produce(salted(1, 9, true)),
    ]
}

/// The fires of each operation of [`replay_script`]. The guarded waiter parks on 1. The peek of
/// the first 2 and its second send fire `p` and `g` and retire the second 2. `a` takes the class
/// of the two 5s, and `q` peeks 1 and stores it again. The persistent 7 fires `t` and is stored.
/// The drain takes 1 and parks, and the last persistent 1 fires the drain as a divergent pair.
const REPLAY_FIRES: [usize; 14] = [0, 0, 0, 0, 0, 0, 2, 1, 1, 0, 0, 1, 1, 1];

/// Runs the replay script on `rig`.
async fn run_replay_script(rig: &Rig) -> Result<Vec<SpaceOutcome>, SpaceError> {
    let mut outcomes = Vec::new();
    for op in replay_script() {
        outcomes.push(rig.run(op).await?);
    }
    Ok(outcomes)
}

/// Replay stages the transactions of play: equal outcomes, every recorded COMM paired (the class
/// fires and the K_retire COMMs included), and the root of play. A replay without the log
/// diverges.
#[tokio::test]
async fn space_set_discipline_replay_equal() {
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
    assert!(
        log.iter().any(|event| matches!(
            event,
            Event::Comm(comm) if comm.consume.channel_hashes.len() == 2
        )),
        "the log holds the class fire over [D; 2]"
    );
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

/// A replay whose log lacks a COMM of a set operation rejects the operation with
/// `ReplayDivergence`. A class fire is the first staged operation of its receive. So the rejected
/// receive applies nothing, and the replay state stays the state of its start. A K_retire
/// follows the fire of its send, so the rejected send keeps that prefix (D6). The revert of the
/// soft checkpoint of the deploy restores the start state.
#[tokio::test]
async fn space_set_discipline_rejected_replay_root_unchanged() {
    let (play, replay) = play_and_replay().await;
    let budget = budget();
    let play_rig = Rig::over(play.clone(), &budget).await;
    let stored = ordered_sends(&[(1, false), (2, false), (2, false), (3, false)]);
    for send in &stored {
        play_rig.send(send.clone()).await;
    }
    play_rig
        .receive(guarded(receiving("w", any()), equals(3)))
        .await;
    assert_eq!(
        play_rig.waiters().await,
        vec!["w".to_string()],
        "w meets 1 first"
    );
    let start = play.create_checkpoint().await.expect("a checkpoint").root;
    let class_receive = receiving("k", exactly(2));
    let class_fire = play_rig.receive(class_receive.clone()).await;
    assert_eq!(class_fire.fires.len(), 1);
    let incoming = salted(3, 77, false);
    let retiring = play_rig.send(incoming.clone()).await;
    assert_eq!(fired(&retiring), vec![delivery("w", &[3])]);
    let Checkpoint { root, log } = play.create_checkpoint().await.expect("a checkpoint");
    let class_consume = Consume::create(
        &vec![data(); 2],
        &vec![class_receive.pattern.clone(); 2],
        &parked(&class_receive),
        false,
    );
    let retire = retire_consume(
        &record::wrap_datum(0, &incoming.data),
        &record_hash(&stored[3]),
    );
    let without = |dropped: &Consume| -> Vec<Event> {
        log.iter()
            .filter(|event| !matches!(event, Event::Comm(comm) if comm.consume == *dropped))
            .cloned()
            .collect()
    };
    assert_eq!(without(&class_consume).len() + 1, log.len());
    assert_eq!(without(&retire).len() + 1, log.len());
    let replay_rig = Rig {
        host: play.clone(),
        env: env_over(replay.clone(), &budget),
    };

    replay
        .rig_and_reset(start.clone(), without(&class_consume))
        .await
        .expect("the replay rigs the log without the class COMM");
    let before = store_bytes(&replay).await;
    match replay_rig
        .run(SpaceOp::Consume(class_receive.clone()))
        .await
    {
        Err(SpaceError::ReplayDivergence(_)) => {}
        other => panic!("the class fire diverges: {other:?}"),
    }
    assert_eq!(
        store_bytes(&replay).await,
        before,
        "the rejected receive applies nothing"
    );
    assert_eq!(
        replay.get_root().await,
        start,
        "the root stays the start root"
    );

    replay
        .rig_and_reset(start.clone(), without(&retire))
        .await
        .expect("the replay rigs the log without the K_retire COMM");
    replay_rig
        .run(SpaceOp::Consume(class_receive.clone()))
        .await
        .expect("the class fire replays");
    let soft = replay.create_soft_checkpoint().await;
    let before = store_bytes(&replay).await;
    match replay_rig.run(SpaceOp::Produce(incoming.clone())).await {
        Err(SpaceError::ReplayDivergence(_)) => {}
        other => panic!("the K_retire diverges: {other:?}"),
    }
    assert_ne!(
        store_bytes(&replay).await,
        before,
        "the paired fire of the waiter stays applied (D6)"
    );
    replay
        .revert_to_soft_checkpoint(soft)
        .await
        .expect("the revert of the deploy");
    assert_eq!(
        store_bytes(&replay).await,
        before,
        "the revert restores the state"
    );

    replay
        .rig_and_reset(start, log)
        .await
        .expect("the replay rigs the full log");
    for op in [SpaceOp::Consume(class_receive), SpaceOp::Produce(incoming)] {
        replay_rig.run(op).await.expect("the full log replays");
    }
    replay
        .check_replay_data()
        .await
        .expect("every recorded COMM is paired");
    assert_eq!(
        replay.create_checkpoint().await.expect("a checkpoint").root,
        root
    );
}

/// In a legacy deploy, each set operation pays what `ChargingRSpace` charges for its host
/// program, entry by entry (decision 45, Option P and Rule E). This includes the class fires over
/// `[D; n]`, the K_retire consumes, and the second send of each peek with its fires.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_set_discipline_legacy_charges_equal_host() {
    let sends = ordered_sends(&[(1, false), (2, false), (2, false), (8, false), (8, false)]);
    // The persistent 6 follows the other 6 in canonical order, so a host consume over [D; 2]
    // binds both (see `HostClass`).
    let sixes = ordered_sends(&[(6, false), (6, true)]);
    // No persistent receive meets a persistent record, so the host program ends.
    let ops = vec![
        SpaceOp::Produce(sends[0].clone()),
        SpaceOp::Produce(sends[3].clone()),
        SpaceOp::Produce(sends[4].clone()),
        SpaceOp::Consume(guarded(receiving("g8", any()), equals(8))),
        SpaceOp::Produce(salted(8, 7, false)),
        SpaceOp::Produce(sends[1].clone()),
        SpaceOp::Produce(sends[2].clone()),
        SpaceOp::Consume(guarded(receiving("g", any()), equals(2))),
        SpaceOp::Consume(peeking(receiving("p", exactly(2)))),
        SpaceOp::Produce(salted(4, 1, false)),
        SpaceOp::Produce(salted(4, 2, false)),
        SpaceOp::Produce(salted(4, 3, false)),
        SpaceOp::Consume(receiving("w", exactly(4))),
        SpaceOp::Consume(peeking(receiving("q", any()))),
        SpaceOp::Consume(receiving("r", exactly(6))),
        SpaceOp::Produce(sixes[1].clone()),
        SpaceOp::Produce(sixes[0].clone()),
        SpaceOp::Consume(receiving("s", exactly(6))),
        SpaceOp::Produce(salted(5, 1, false)),
        SpaceOp::Consume(persisting(receiving("drain", exactly(5)))),
        SpaceOp::Produce(salted(5, 2, false)),
        SpaceOp::Consume(guarded(receiving("refuses", any()), refusing())),
        SpaceOp::Produce(salted(7, 1, false)),
    ];
    assert!(
        compare_with_host(&ops)
            .await
            .unwrap_or_else(|failure| panic!("{failure}")),
        "every operation has its host program"
    );
}

/// A budget one unit below the usage of a set operation in any dimension rejects the operation
/// before the commit, so it has no effect. The exact budget gives the outcome of an unlimited
/// one. The operations are a send that fires a guarded waiter and retires a class of two, and a
/// class fire over `[D; 3]` (decision 42).
#[tokio::test]
async fn space_set_discipline_reservation_cut_rejects_without_effect() {
    let setup = || async {
        let rig = Rig::new().await;
        for send in ordered_sends(&[(1, false), (8, false), (8, false)]) {
            rig.send(send).await;
        }
        for salt in 1..=3 {
            rig.send(salted(4, salt, false)).await;
        }
        rig.receive(guarded(receiving("g", any()), equals(8))).await;
        rig.log().await;
        rig
    };
    let operations = [
        SpaceOp::Produce(salted(8, 50, false)),
        SpaceOp::Consume(receiving("k", exactly(4))),
    ];
    for op in operations {
        let measured = unbounded_host_work();
        let unlimited = setup().await.metered(&measured);
        let expected = unlimited
            .run(op.clone())
            .await
            .expect("the operation commits");
        assert_eq!(expected.fires.len(), 1);
        let mut exact = HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX));
        for dimension in AGENT_DIMENSIONS {
            exact.set(
                dimension,
                HostWorkLimit::new(measured.usage(dimension).get()),
            );
        }
        let fitting = setup().await.metered(&HostWorkBudget::new(exact));
        assert_eq!(fitting.run(op.clone()).await, Ok(expected));
        for dimension in AGENT_DIMENSIONS {
            let usage = measured.usage(dimension).get();
            assert!(usage > 0, "the operation charges {dimension}");
            let mut cut = exact;
            cut.set(dimension, HostWorkLimit::new(usage - 1));
            let rig = setup().await.metered(&HostWorkBudget::new(cut));
            let (values, waiters) = (rig.values().await, rig.waiters().await);
            assert_eq!(
                rig.run(op.clone()).await,
                Err(SpaceError::Host(RSpaceError::HostWorkRejected)),
                "{dimension}"
            );
            assert!(rig.log().await.is_empty(), "{dimension}");
            assert_eq!(rig.values().await, values, "{dimension}");
            assert_eq!(rig.waiters().await, waiters, "{dimension}");
        }
    }
}

/// A re-issued persistent operation keeps the record and the identity of its first round
/// (decision 43). Every `ord` of a set is 0, and a re-issue carries no order. The test checks
/// the round cap and a divergent pair of a persistent send. It also checks a divergent pair of a
/// persistent receive whose class has a persistent member.
#[tokio::test]
async fn space_set_discipline_persistent_reissue_keeps_identity() {
    let rig = Rig::new().await;
    for index in 0..=FIRE_ROUND_CAP {
        rig.receive(receiving(&format!("w{index}"), any())).await;
    }
    rig.log().await;
    let forever = salted(1, 1, true);
    let first = rig.send(forever.clone()).await;
    assert_eq!(first.fires.len(), FIRE_ROUND_CAP);
    assert_eq!(
        first.reissue,
        Some(SpaceReissue {
            op: SpaceOp::Produce(ProduceOp {
                reissued_ord: None,
                ..forever.clone()
            }),
            cause: ReissueCause::RoundCap,
        })
    );
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
    assert_eq!(rig.values().await, vec![(1, true)]);

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

    let rig = Rig::new().await;
    rig.send(salted(6, 1, true)).await;
    rig.log().await;
    let receive = persisting(receiving("s", exactly(6)));
    let mut next = Some(SpaceOp::Consume(receive.clone()));
    let mut rounds = Vec::new();
    for _ in 0..3 {
        let op = next.take().expect("a divergent pair re-issues");
        let outcome = rig.run(op).await.expect("the round commits");
        assert_eq!(fired(&outcome), vec![delivery("s", &[])]);
        let reissue = outcome.reissue.expect("the persistent member stays");
        assert_eq!(reissue.cause, ReissueCause::DivergentPair);
        assert_eq!(
            reissue.op,
            SpaceOp::Consume(receive.clone()),
            "no carried order"
        );
        next = Some(reissue.op);
        rounds.push(comms(&rig.log().await));
    }
    assert!(
        rounds
            .iter()
            .all(|round| round.len() == 1 && round[0] == rounds[0][0]),
        "every round of the receive repeats one COMM"
    );
    assert_eq!(rig.values().await, vec![(6, true)]);
    assert!(rig.waiters().await.is_empty(), "the receive is in flight");
}

// ─── Property tests: the headlines of SetDiscipline.v and V6 of GenericRSpace.tla ────────

/// A failure of a property case with `message`.
fn failure(message: impl Into<String>) -> TestCaseError { TestCaseError::fail(message.into()) }

/// A count of the cases of a property that reached one behavior. A property asserts a least
/// count after its run, so a property that never reaches its behavior fails instead of passing
/// with no evidence.
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

/// A `Par` with the integer expressions `values`, in this order.
fn exprs_par(values: &[i64]) -> Par {
    Par {
        exprs: values
            .iter()
            .map(|value| Expr {
                expr_instance: Some(ExprInstance::GInt(*value)),
            })
            .collect(),
        ..Par::default()
    }
}

/// A list of the integers `values`.
fn list_par(values: &[i64]) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                ps: values.iter().map(|value| int(*value)).collect(),
                ..EList::default()
            })),
        }],
        ..Par::default()
    }
}

/// The sorted `Par` of `element`, as the host sorts it.
fn sorted(element: &Par) -> Par { ParSortMatcher::sort_match(element).term }

/// A generated element of a payload: a number, a string, a `Par` of several expressions in a
/// generated order, or a list.
fn arb_element() -> BoxedStrategy<Par> {
    prop_oneof![
        (0i64..3).prop_map(int),
        (0u8..3).prop_map(|value| string(&format!("s{value}"))),
        vec(0i64..3, 2..=3).prop_map(|values| exprs_par(&values)),
        vec(0i64..3, 0..=2).prop_map(|values| list_par(&values)),
    ]
    .boxed()
}

/// `payload` with the expressions of each element in reverse order: the same sorted payload.
fn reordered(payload: &[Par]) -> Vec<Par> {
    payload
        .iter()
        .map(|element| {
            let mut element = element.clone();
            element.exprs.reverse();
            element
        })
        .collect()
}

/// The elements of a set key: its count and the bytes of each element, or `None` when the
/// framing does not hold.
fn parse_key(key: &[u8]) -> Option<Vec<Vec<u8>>> {
    let field = |bytes: &[u8]| -> Option<usize> {
        let array: [u8; LENGTH_BYTES] = bytes.get(..LENGTH_BYTES)?.try_into().ok()?;
        usize::try_from(u64::from_be_bytes(array)).ok()
    };
    let count = field(key)?;
    let mut rest = &key[LENGTH_BYTES..];
    let mut elements = Vec::with_capacity(count);
    for _ in 0..count {
        let length = field(rest)?;
        rest = &rest[LENGTH_BYTES..];
        elements.push(rest.get(..length)?.to_vec());
        rest = &rest[length..];
    }
    match rest.is_empty() {
        true => Some(elements),
        false => None,
    }
}

/// F6 `set_key_injective`: two payloads have equal keys exactly when their sorted elements are
/// equal. The key frames the count and each length around the bincode of each sorted element,
/// and the metered key has the same bytes.
#[test]
fn prop_set_key_injective() {
    let equal = Hits::new();
    let strategy = (vec(arb_element(), 0..4), 0u8..4, vec(arb_element(), 0..4)).prop_map(
        |(payload, kind, other)| {
            let second = match kind {
                0 => reordered(&payload),
                1 => payload.iter().rev().cloned().collect(),
                2 => payload
                    .iter()
                    .take(payload.len().saturating_sub(1))
                    .cloned()
                    .collect(),
                _ => other,
            };
            (payload, second)
        },
    );
    check(pure_config(), strategy, |(first, second)| {
        let meter = ScanMeter::new(None);
        let key = |payload: &[Par]| set_key(payload, &meter).expect("a key");
        let (first_key, second_key) = (key(&first), key(&second));
        let sorted_first: Vec<Par> = first.iter().map(sorted).collect();
        let sorted_second: Vec<Par> = second.iter().map(sorted).collect();
        if sorted_first == sorted_second {
            equal.hit();
        }
        prop_assert_eq!(first_key == second_key, sorted_first == sorted_second);
        for (payload, key) in [(&first, &first_key), (&second, &second_key)] {
            let elements = parse_key(key);
            prop_assert!(elements.is_some(), "the framing holds");
            let expected: Vec<Vec<u8>> = payload
                .iter()
                .map(|element| bincode::serialize(&sorted(element)).expect("a Par serializes"))
                .collect();
            prop_assert_eq!(elements.expect("checked above"), expected);
            let metered = set_key(payload, &ScanMeter::new(Some(unbounded_host_work())));
            prop_assert_eq!(metered.as_ref(), Ok(key));
        }
        Ok(())
    });
    equal.assert_at_least(8, "two payloads with one sorted form");
}

/// Records of generated values and salts. A pair with one value and one salt is a pair of
/// byte-identical records.
fn arb_records(max: usize) -> BoxedStrategy<Vec<(i64, u64)>> {
    vec((0i64..4, 0u64..1_000), 1..=max).boxed()
}

/// F7 `set_view_dedups_equal_pars`: receives of any value drain the set class by class. Each
/// fire dispatches the first record in canonical order, the representative of its class, and
/// removes the whole class. The fires are one for each class, and the drain is the same for
/// every insertion order (decision 41).
#[test]
fn prop_set_view_dedups_equal_pars() {
    let strategy = arb_records(7)
        .prop_flat_map(|records| (Just(records.clone()), Just(records).prop_shuffle()));
    check(pure_config(), strategy, |(records, shuffled)| {
        tokio_runtime().block_on(view_case(records, shuffled))
    });
}

async fn view_case(
    records: Vec<(i64, u64)>,
    shuffled: Vec<(i64, u64)>,
) -> Result<(), TestCaseError> {
    let classes: BTreeSet<i64> = records.iter().map(|(value, _)| *value).collect();
    let mut drains = Vec::with_capacity(2);
    for order in [&records, &shuffled] {
        let rig = Rig::new().await;
        for (value, salt) in order {
            rig.send(salted(*value, *salt, false)).await;
        }
        let mut drained = Vec::new();
        for index in 0.. {
            let first = canonical_records(&rig.host).await.first().cloned();
            let outcome = rig.receive(receiving(&format!("r{index}"), any())).await;
            let Some(first) = first else {
                prop_assert!(outcome.fires.is_empty(), "an empty set parks the receive");
                break;
            };
            prop_assert_eq!(outcome.fires.len(), 1);
            prop_assert_eq!(&removed(&outcome, 0), &first.a, "the representative fires");
            let value = record_value(&first.a);
            prop_assert!(
                rig.values()
                    .await
                    .iter()
                    .all(|(stored, _)| *stored != value),
                "the class leaves at once"
            );
            drained.push(first.source.hash);
        }
        prop_assert_eq!(drained.len(), classes.len(), "one fire for each class");
        drains.push(drained);
    }
    prop_assert_eq!(
        &drains[0],
        &drains[1],
        "the insertion order changes nothing"
    );
    Ok(())
}

/// F8 `set_consume_retires_class`: a receive that does not peek takes the first record in
/// canonical order that its pattern binds, and it checks its guard once. Its COMM binds every
/// member of the class in canonical order, the representative first. The fire removes every
/// non-persistent member and keeps the persistent members and the other classes.
#[test]
fn prop_set_consume_retires_class() {
    let classes = Hits::new();
    let strategy = (
        vec((0i64..3, 0u64..1_000, proptest::bool::weighted(0.25)), 1..8),
        arb_pat(),
    )
        .prop_flat_map(|(records, pat)| (Just(records), Just(pat), arb_guard(pat)));
    check(pure_config(), strategy, |(records, pat, guard)| {
        tokio_runtime().block_on(consume_case(records, pat, guard, &classes))
    });
    classes.assert_at_least(16, "a class fire of two or more members");
}

async fn consume_case(
    records: Vec<(i64, u64, bool)>,
    pat: Pat,
    guard: Guard,
    classes: &Hits,
) -> Result<(), TestCaseError> {
    let rig = Rig::new().await;
    for (value, salt, persistent) in &records {
        rig.send(salted(*value, *salt, *persistent)).await;
    }
    let before = canonical_records(&rig.host).await;
    let values_before = rig.values().await;
    rig.log().await;
    let outcome = rig
        .receive(ConsumeOp {
            guard: guard.par(),
            ..receiving("k", pat.pattern())
        })
        .await;
    let log = rig.log().await;
    let first = before
        .iter()
        .find(|stored| pat.fits(record_value(&stored.a)));
    match first {
        Some(representative) if guard.admits(record_value(&representative.a)) => {
            let value = record_value(&representative.a);
            let members: Vec<&Datum<ListParWithRandom>> = before
                .iter()
                .filter(|stored| record_value(&stored.a) == value)
                .collect();
            if members.len() >= 2 {
                classes.hit();
            }
            prop_assert_eq!(&removed(&outcome, 0), &representative.a);
            let [comm] = <[COMM; 1]>::try_from(comms(&log))
                .map_err(|comms| failure(format!("one COMM: {comms:?}")))?;
            let listed: Vec<Blake2b256Hash> = comm
                .produces
                .iter()
                .map(|produce| produce.hash.clone())
                .collect();
            let expected: Vec<Blake2b256Hash> = members
                .iter()
                .map(|member| member.source.hash.clone())
                .collect();
            prop_assert_eq!(listed, expected, "every member, in canonical order");
            let mut expected_values: Vec<(i64, bool)> = values_before
                .iter()
                .filter(|(stored, persistent)| *stored != value || *persistent)
                .cloned()
                .collect();
            expected_values.sort();
            prop_assert_eq!(rig.values().await, expected_values);
            prop_assert!(rig.waiters().await.is_empty());
        }
        _ => {
            prop_assert!(
                outcome.fires.is_empty(),
                "no match, or the guard refuses it"
            );
            prop_assert_eq!(rig.values().await, values_before);
            prop_assert_eq!(rig.waiters().await, vec!["k".to_string()]);
        }
    }
    Ok(())
}

/// The K_retire consumes that an operation can stage. An element that can fire is the datum of a
/// send or a stored record that a peek sends again. For each such element and each stored
/// non-persistent member, the result holds the consume of the retire continuation.
fn possible_retires(op: &SpaceOp, before: &[Datum<ListParWithRandom>]) -> BTreeSet<Blake2b256Hash> {
    let mut elements: Vec<ListParWithRandom> =
        before.iter().map(|stored| stored.a.clone()).collect();
    if let SpaceOp::Produce(send) = op {
        elements.push(record::wrap_datum(0, &send.data));
    }
    let mut retires = BTreeSet::new();
    for element in &elements {
        for member in before.iter().filter(|stored| !stored.persist) {
            retires.insert(retire_consume(element, &member.source.hash).hash);
        }
    }
    retires
}

/// F9 `set_retire_comm_lines_up`: over generated programs, every COMM of a set lines up one
/// channel with each bound record. Its group is the data channel repeated. A record is never
/// bound more often than it is stored. Every K_retire binds one stored non-persistent member.
/// The outcome dispatches one result for each fire and none for a retire.
#[test]
fn prop_set_retire_comm_lines_up() {
    let groups = Hits::new();
    let retires = Hits::new();
    check(pure_config(), arb_any_program(), |program| {
        tokio_runtime().block_on(lines_up_case(program, &groups, &retires))
    });
    groups.assert_at_least(16, "a class fire over [D; n] with n of two or more");
    retires.assert_at_least(8, "a K_retire");
}

async fn lines_up_case(
    program: Vec<SetOp>,
    groups: &Hits,
    retire_hits: &Hits,
) -> Result<(), TestCaseError> {
    let rig = Rig::new().await;
    let data_hash = stable_hash_provider::hash(&data());
    for (index, op) in program.iter().enumerate() {
        let space_op = op.space_op();
        let before = rig.records().await;
        let retires = possible_retires(&space_op, &before);
        let outcome = rig
            .run(space_op.clone())
            .await
            .map_err(|error| failure(format!("operation {index} fails: {error}")))?;
        let log = rig.log().await;
        let mut stored: BTreeMap<Blake2b256Hash, usize> = BTreeMap::new();
        for record in &before {
            *stored.entry(record.source.hash.clone()).or_default() += 1;
        }
        let mut dispatched = 0;
        for comm in comms(&log) {
            let width = comm.consume.channel_hashes.len();
            prop_assert!(width >= 1);
            prop_assert_eq!(width, comm.produces.len(), "one record for each channel");
            prop_assert!(comm
                .consume
                .channel_hashes
                .iter()
                .all(|hash| *hash == data_hash));
            if width >= 2 {
                groups.hit();
            }
            match retires.contains(&comm.consume.hash) {
                true => {
                    retire_hit_check(&comm, &before)?;
                    retire_hits.hit();
                }
                false => dispatched += 1,
            }
            let mut bound: BTreeMap<Blake2b256Hash, usize> = BTreeMap::new();
            for produce in &comm.produces {
                *bound.entry(produce.hash.clone()).or_default() += 1;
            }
            for (hash, count) in bound {
                let available = stored.get(&hash).copied().unwrap_or(0) + 1;
                prop_assert!(count <= available, "a record is bound once for each copy");
            }
        }
        prop_assert_eq!(
            outcome.fires.len(),
            dispatched,
            "a retire is never dispatched"
        );
        prop_assert!(outcome.fires.iter().all(|fire| fire.results.len() == 1));
    }
    Ok(())
}

/// A K_retire COMM binds one stored non-persistent member.
fn retire_hit_check(comm: &COMM, before: &[Datum<ListParWithRandom>]) -> Result<(), TestCaseError> {
    let [produce] = comm.produces.as_slice() else {
        return Err(failure("a K_retire binds one record"));
    };
    prop_assert!(
        before
            .iter()
            .any(|stored| !stored.persist && stored.source.hash == produce.hash),
        "a K_retire binds a stored non-persistent member"
    );
    prop_assert!(comm.peeks.is_empty());
    Ok(())
}

/// The guard values and the peek flags of the guarded waiters of a generated setup. The waiter
/// at index `i` has the body name `g{i}`.
type Waiters = Vec<(i64, bool)>;

/// A generated setup of a set: stored records, a choice of the target class, the peek flag of
/// the target waiter, and more guarded waiters.
#[derive(Clone, Debug)]
struct ClassSetup {
    records: Vec<(i64, u64, bool)>,
    target: usize,
    target_peeks: bool,
    extra: Waiters,
}

fn arb_class_setup() -> BoxedStrategy<ClassSetup> {
    (
        vec((0i64..3, 0u64..1_000, proptest::bool::weighted(0.2)), 2..7),
        0usize..3,
        proptest::bool::weighted(0.25),
        vec((0i64..3, proptest::bool::weighted(0.3)), 0..3),
    )
        .prop_map(|(records, target, target_peeks, extra)| ClassSetup {
            records,
            target,
            target_peeks,
            extra,
        })
        .boxed()
}

/// Stores the records of `setup`, parks a guarded waiter `x == v` for a target value `v`, and
/// parks the extra guarded waiters. The target is a stored value other than the value of the
/// first record in canonical order. So its waiter meets a refused record first and parks
/// (decision 37). It returns the rig, all the waiters (the target first) and the target value.
/// When every stored record has one value, the target is a value that no record has.
async fn class_setup(setup: &ClassSetup) -> (Rig, Waiters, i64) {
    let rig = Rig::new().await;
    for (value, salt, persistent) in &setup.records {
        rig.send(salted(*value, *salt, *persistent)).await;
    }
    let first = record_value(&canonical_records(&rig.host).await[0].a);
    let others: Vec<i64> = rig
        .values()
        .await
        .into_iter()
        .map(|(value, _)| value)
        .filter(|value| *value != first)
        .collect::<BTreeSet<i64>>()
        .into_iter()
        .collect();
    let target = match others.as_slice() {
        [] => first + 10,
        values => values[setup.target % values.len()],
    };
    let mut waiters = Vec::with_capacity(1 + setup.extra.len());
    waiters.push((target, setup.target_peeks));
    waiters.extend(setup.extra.iter().copied());
    for (index, (value, peek)) in waiters.iter().enumerate() {
        rig.receive(ConsumeOp {
            peek: *peek,
            ..guarded(receiving(&format!("g{index}"), any()), equals(*value))
        })
        .await;
    }
    rig.log().await;
    (rig, waiters, target)
}

/// The peek flag of the guarded waiter with the body name `name` in `waiters`.
fn waiter_peeks(waiters: &Waiters, name: &str) -> Result<bool, TestCaseError> {
    let index: usize = name
        .strip_prefix('g')
        .and_then(|index| index.parse().ok())
        .ok_or_else(|| failure(format!("a fired guarded waiter: {name}")))?;
    waiters
        .get(index)
        .map(|(_, peek)| *peek)
        .ok_or_else(|| failure("a known waiter"))
}

/// F10 `set_produce_fire_retires_stored_class`: a send that fires a guarded waiter that does
/// not peek retires the stored non-persistent members of its class, with one K_retire each. The
/// class keeps its persistent members only. A send whose fires all peek retires nothing.
#[test]
fn prop_set_produce_fire_retires_stored_class() {
    let retired = Hits::new();
    let peeked = Hits::new();
    let strategy = (arb_class_setup(), 1_000u64..2_000);
    check(pure_config(), strategy, |(setup, salt)| {
        tokio_runtime().block_on(produce_retire_case(setup, salt, &retired, &peeked))
    });
    retired.assert_at_least(24, "a send that retires a stored member");
    peeked.assert_at_least(4, "a send whose fires all peek");
}

async fn produce_retire_case(
    setup: ClassSetup,
    salt: u64,
    retired: &Hits,
    peeked: &Hits,
) -> Result<(), TestCaseError> {
    let (rig, waiters, value) = class_setup(&setup).await;
    let members_before: Vec<Datum<ListParWithRandom>> = rig
        .records()
        .await
        .into_iter()
        .filter(|stored| record_value(&stored.a) == value)
        .collect();
    let incoming = salted(value, salt, false);
    let outcome = rig.send(incoming.clone()).await;
    let log = rig.log().await;
    let mut non_peek = false;
    for (name, _) in fired(&outcome) {
        non_peek |= !waiter_peeks(&waiters, &name)?;
    }
    let element = record::wrap_datum(0, &incoming.data);
    let retire_hashes: BTreeSet<Blake2b256Hash> = members_before
        .iter()
        .map(|member| retire_consume(&element, &member.source.hash).hash)
        .collect();
    let retire_comms = comms(&log)
        .iter()
        .filter(|comm| retire_hashes.contains(&comm.consume.hash))
        .count();
    let transient = members_before
        .iter()
        .filter(|stored| !stored.persist)
        .count();
    let class_after: Vec<(i64, bool)> = rig
        .values()
        .await
        .into_iter()
        .filter(|(stored, _)| *stored == value)
        .collect();
    match non_peek {
        true => {
            if transient > 0 {
                retired.hit();
            }
            prop_assert_eq!(retire_comms, transient, "one K_retire for each member");
            prop_assert!(
                class_after.iter().all(|(_, persistent)| *persistent),
                "no non-persistent member stays"
            );
        }
        false => {
            if !outcome.fires.is_empty() {
                peeked.hit();
            }
            prop_assert_eq!(retire_comms, 0, "a peek fire retires nothing");
            prop_assert_eq!(
                class_after.len(),
                members_before.len() + 1,
                "the class keeps its members, and the datum is stored"
            );
        }
    }
    Ok(())
}

/// `set_peek_restores_representative_only` (Rule E): a peek binds the representative alone,
/// the first record in canonical order that its pattern binds. A non-persistent representative
/// is sent again right after the fire, with its identity. Without a later fire the records stay
/// as they were. A K_retire appears only after a fire of the second send on a waiter that does
/// not peek. Then the class keeps its persistent members only.
#[test]
fn prop_set_peek_restores_representative_only() {
    let reoffers = Hits::new();
    let restored = Hits::new();
    let strategy = (arb_class_setup(), prop_oneof![
        2 => Just(None),
        1 => arb_pat().prop_map(Some),
    ]);
    check(pure_config(), strategy, |(setup, pat)| {
        tokio_runtime().block_on(peek_case(setup, pat, &reoffers, &restored))
    });
    reoffers.assert_at_least(
        16,
        "a second send that fires a waiter and retires the class",
    );
    restored.assert_at_least(8, "a second send that stores the record again");
}

async fn peek_case(
    setup: ClassSetup,
    pat: Option<Pat>,
    reoffers: &Hits,
    restored: &Hits,
) -> Result<(), TestCaseError> {
    let (rig, waiters, target) = class_setup(&setup).await;
    let pat = pat.unwrap_or(Pat::Lit(target));
    let before = canonical_records(&rig.host).await;
    let values_before = rig.values().await;
    let outcome = rig.receive(peeking(receiving("p", pat.pattern()))).await;
    let log = rig.log().await;
    let Some(representative) = before
        .iter()
        .find(|stored| pat.fits(record_value(&stored.a)))
    else {
        prop_assert!(outcome.fires.is_empty());
        prop_assert_eq!(rig.values().await, values_before);
        return Ok(());
    };
    prop_assert_eq!(&removed(&outcome, 0), &representative.a);
    let Some(Event::Comm(peek_comm)) = log.get(1) else {
        return Err(failure(format!(
            "the peek COMM follows its consume: {log:?}"
        )));
    };
    prop_assert_eq!(
        peek_comm.consume.channel_hashes.len(),
        1,
        "the representative alone"
    );
    prop_assert_eq!(
        peek_comm
            .produces
            .iter()
            .map(|produce| produce.hash.clone())
            .collect::<Vec<_>>(),
        vec![representative.source.hash.clone()]
    );
    prop_assert!(!peek_comm.peeks.is_empty());
    if representative.persist {
        prop_assert_eq!(
            log.len(),
            2,
            "a persistent representative is not sent again"
        );
        prop_assert_eq!(rig.values().await, values_before);
        return Ok(());
    }
    match log.get(2) {
        Some(Event::IoEvent(IOEvent::Produce(second))) => {
            prop_assert_eq!(&second.hash, &representative.source.hash, "the second send");
        }
        other => {
            return Err(failure(format!(
                "the second send follows the peek: {other:?}"
            )))
        }
    }
    let value = record_value(&representative.a);
    let element = representative.a.clone();
    let retire_hashes: BTreeSet<Blake2b256Hash> = before
        .iter()
        .map(|member| retire_consume(&element, &member.source.hash).hash)
        .collect();
    let retire_comms = comms(&log)
        .iter()
        .filter(|comm| retire_hashes.contains(&comm.consume.hash))
        .count();
    let later: Vec<(String, Vec<Par>)> = fired(&outcome).into_iter().skip(1).collect();
    let mut non_peek = false;
    for (name, _) in &later {
        non_peek |= !waiter_peeks(&waiters, name)?;
    }
    match (later.is_empty(), non_peek) {
        (true, _) => {
            restored.hit();
            prop_assert_eq!(retire_comms, 0);
            prop_assert_eq!(
                rig.values().await,
                values_before,
                "the record is stored again"
            );
        }
        (false, true) => {
            reoffers.hit();
            prop_assert!(
                rig.values()
                    .await
                    .iter()
                    .all(|(stored, persistent)| *stored != value || *persistent),
                "the class keeps its persistent members only"
            );
        }
        (false, false) => prop_assert_eq!(retire_comms, 0, "peek fires retire nothing"),
    }
    Ok(())
}

/// `set_membership_full_compare_on_equal_hash` (DR-90): two keys are one class exactly when
/// they are equal, and their metered hashes are equal exactly then. A class fire binds exactly
/// the records whose sorted payloads equal the sorted payload of the representative, also when
/// their expressions come in other orders.
#[test]
fn prop_set_membership_full_compare_on_equal_hash() {
    let permuted = Hits::new();
    let multisets: [&[i64]; 4] = [&[1, 2], &[1, 2, 2], &[2], &[2, 1, 1]];
    let strategy = vec(
        (0usize..4, 0u64..1_000).prop_flat_map(move |(class, salt)| {
            (
                Just(class),
                Just(multisets[class].to_vec()).prop_shuffle(),
                Just(salt),
            )
        }),
        1..7,
    );
    check(pure_config(), strategy, |records| {
        tokio_runtime().block_on(membership_case(records, &permuted))
    });
    permuted.assert_at_least(8, "a class with two expression orders");
}

async fn membership_case(
    records: Vec<(usize, Vec<i64>, u64)>,
    permuted: &Hits,
) -> Result<(), TestCaseError> {
    let meter = ScanMeter::new(None);
    let metered = ScanMeter::new(Some(unbounded_host_work()));
    let payload = |values: &[i64]| vec![exprs_par(values)];
    for (first_class, first, _) in &records {
        for (second_class, second, _) in &records {
            let first_key = set_key(&payload(first), &meter).expect("a key");
            let second_key = set_key(&payload(second), &meter).expect("a key");
            let same = first_class == second_class;
            prop_assert_eq!(first_key == second_key, same);
            let first_hash = metered_hash(&first_key, &metered).expect("a metered hash");
            let second_hash = metered_hash(&second_key, &metered).expect("a metered hash");
            prop_assert_eq!(first_hash == second_hash, same);
            if same && first != second {
                permuted.hit();
            }
        }
    }
    let rig = Rig::new().await;
    for (index, (_, values, salt)) in records.iter().enumerate() {
        rig.send(send_pars(payload(values), *salt + 10_000 * index as u64))
            .await;
    }
    let before = canonical_records(&rig.host).await;
    rig.log().await;
    let outcome = rig.receive(receiving("k", any())).await;
    let representative = &before[0];
    prop_assert_eq!(&removed(&outcome, 0), &representative.a);
    let representative_key = key_of_record(&representative.a);
    let members: Vec<Blake2b256Hash> = before
        .iter()
        .filter(|stored| key_of_record(&stored.a) == representative_key)
        .map(|stored| stored.source.hash.clone())
        .collect();
    let [comm] = <[COMM; 1]>::try_from(comms(&rig.log().await))
        .map_err(|comms| failure(format!("one COMM: {comms:?}")))?;
    prop_assert_eq!(
        comm.produces
            .iter()
            .map(|produce| produce.hash.clone())
            .collect::<Vec<_>>(),
        members
    );
    Ok(())
}

/// `set_key_trace_covered` (decision 42): a generated program runs first. Then a budget one
/// unit below the usage of the next operation in any dimension rejects that operation before the
/// commit, with no effect. The exact budget gives the outcome of an unlimited one. The programs
/// make class fires, K_retire consumes and peeks.
#[test]
fn prop_set_key_trace_covered() {
    let strategy = (arb_class_program(), arb_program(1));
    check(pure_config(), strategy, |(setup, last)| {
        tokio_runtime().block_on(cut_case(setup, last))
    });
}

async fn cut_case(setup: Vec<SetOp>, last: Vec<SetOp>) -> Result<(), TestCaseError> {
    let op = match last.into_iter().next() {
        Some(SetOp::Receive {
            pat,
            guard,
            persistent,
            peek,
            ..
        }) => SetOp::Receive {
            name: 100,
            pat,
            guard,
            persistent,
            peek,
        },
        Some(send) => send,
        None => return Err(failure("a program of one operation")),
    };
    let prepare = || async {
        let rig = Rig::new().await;
        for step in &setup {
            rig.run(step.space_op())
                .await
                .expect("the setup operation commits");
        }
        rig.log().await;
        rig
    };
    let measured = unbounded_host_work();
    let expected = prepare().await.metered(&measured).run(op.space_op()).await;
    prop_assert!(expected.is_ok(), "the operation commits: {:?}", expected);
    let mut exact = HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX));
    for dimension in AGENT_DIMENSIONS {
        exact.set(
            dimension,
            HostWorkLimit::new(measured.usage(dimension).get()),
        );
    }
    let fitting = prepare().await.metered(&HostWorkBudget::new(exact));
    prop_assert_eq!(&fitting.run(op.space_op()).await, &expected);
    for dimension in AGENT_DIMENSIONS {
        let usage = measured.usage(dimension).get();
        prop_assert!(usage > 0, "the operation charges {}", dimension);
        let mut cut = exact;
        cut.set(dimension, HostWorkLimit::new(usage - 1));
        let rig = prepare().await.metered(&HostWorkBudget::new(cut));
        let (values, waiters) = (rig.values().await, rig.waiters().await);
        prop_assert_eq!(
            rig.run(op.space_op()).await,
            Err(SpaceError::Host(RSpaceError::HostWorkRejected)),
            "{}",
            dimension
        );
        prop_assert!(rig.log().await.is_empty(), "{}", dimension);
        prop_assert_eq!(rig.values().await, values, "{}", dimension);
        prop_assert_eq!(rig.waiters().await, waiters, "{}", dimension);
    }
    Ok(())
}

/// V6 `SetClassRetired` of `GenericRSpace.tla`: over generated programs, the agent makes the
/// fires of the model and keeps its records and waiters. After every operation, no
/// non-persistent record remains of a class that a fire without a peek took or retired. Every
/// COMM that the outcome does not dispatch is a K_retire of the model.
#[test]
fn prop_set_class_retired_any_schedule() {
    let large = Hits::new();
    let produced = Hits::new();
    check(pure_config(), arb_any_program(), |program| {
        tokio_runtime().block_on(class_retired_case(program, &large, &produced))
    });
    large.assert_at_least(16, "a class of two or more members leaves at once");
    // The dedicated property of F10 carries most of the evidence of produce-side retires.
    produced.assert_at_least(
        6,
        "a fire of a send or of a second send that retires stored members",
    );
}

async fn class_retired_case(
    program: Vec<SetOp>,
    large: &Hits,
    produced: &Hits,
) -> Result<(), TestCaseError> {
    let rig = Rig::new().await;
    let mut model = SetModel::default();
    for (index, op) in program.iter().enumerate() {
        let values_before = rig.values().await;
        let predicted = model.apply(op);
        let outcome = rig
            .run(op.space_op())
            .await
            .map_err(|error| failure(format!("operation {index} fails: {error}")))?;
        let log = rig.log().await;
        prop_assert_eq!(
            fired(&outcome),
            predicted.fires.clone(),
            "the fires of {}",
            index
        );
        prop_assert_eq!(&outcome.reissue, &None);
        prop_assert_eq!(
            comms(&log).len() - outcome.fires.len(),
            predicted.retired,
            "the K_retire COMMs of {}",
            index
        );
        let values = rig.values().await;
        prop_assert_eq!(&values, &model.values(), "the records after {}", index);
        prop_assert_eq!(
            rig.waiters().await,
            model.waiter_names(),
            "the waiters after {}",
            index
        );
        for class in &predicted.classes {
            prop_assert!(
                values
                    .iter()
                    .all(|(stored, persistent)| stored != class || *persistent),
                "the class {} keeps no non-persistent record after {}",
                class,
                index
            );
            let members = values_before
                .iter()
                .filter(|(stored, persistent)| stored == class && !persistent)
                .count();
            if members >= 2 {
                large.hit();
            }
        }
        if predicted.retired > 0 {
            produced.hit();
        }
    }
    Ok(())
}

/// Option P (decision 45): over generated programs, in a legacy deploy, each set operation
/// stages the operations of its host program and pays its charges. This includes the class
/// fires, the K_retire consumes, and the second sends of peeks with their fires. The stores end
/// equal.
#[test]
fn prop_set_legacy_charges_equal_host() {
    let compared = Hits::new();
    check(pure_config(), arb_any_program(), |program| {
        let ops: Vec<SpaceOp> = program.iter().map(SetOp::space_op).collect();
        if tokio_runtime().block_on(compare_with_host(&ops))? {
            compared.hit();
        }
        Ok(())
    });
    compared.assert_at_least(48, "a program whose every operation has its host program");
}
