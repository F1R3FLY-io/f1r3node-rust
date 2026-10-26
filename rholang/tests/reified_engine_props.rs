//! The properties of the engine of the native spaces (P5a, batch ENGINE-FORMAL).
//!
//! Each property states one invariant of `GenericRSpace.tla` (V1 to V8) or one headline of
//! `Match.v` or `GenericRSpace.v`, and checks it over random scripts of space operations. The
//! scripts run the real agents of the engine spine S1 over an in-memory host store, through the
//! shared agent environment of `support/`.
//!
//! The families under test:
//!
//! * `hashmap:queue`: the head driver of S1 (`QueueDiscipline`).
//! * `hashmap:bag`: the scan driver of A.7 (`BagDiscipline` and `disciplines::scan`).
//! * `hashmap:cell`: the head driver with a capacity of one entry on each side
//!   (`CellDiscipline`).
//! * `hashmap:set`: the scan driver with the F5 class key of the set (`SetDiscipline` of the SET
//!   batch), so equal payloads form one class.
//!
//! A script is a list of sends, receives and re-entries. A re-entry runs the oldest re-issue of
//! an earlier operation, so other operations can run while an element is in flight. Each send
//! and each receive has its own random state and its own authority, so a test can follow every
//! datum by its random state (its token) and every continuation by its body name.
//!
//! The properties use the fixed seed and the case count of the P5 harness (64 cases).

#[path = "support/mod.rs"]
mod support;

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::cost_signature::Value as CostSignatureValue;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{
    BindPattern, CostAuthority, CostRegion, CostSignature, EEq, EVar, Expr, ListParWithRandom, Par,
    ParWithRandom, TaggedContinuation, Var,
};
use models::rust::space_factory_urn::SpaceFactoryUrn;
use proptest::collection::vec;
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;
use rholang::rust::interpreter::accounting::authority::AuthorityByteEventKind;
use rholang::rust::interpreter::accounting::byte_accounting::produce_introduction_identity;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::spaces::agent::{
    ConsumeOp, JoinOp, ProduceOp, SpaceEnv, SpaceOp, SpaceOutcome,
};
// Disabled with `ProbeSet`, which `SetDiscipline` replaces (SET batch): only `ProbeSet` used it.
// use rholang::rust::interpreter::spaces::agent::SpaceReissue;
use rholang::rust::interpreter::spaces::disciplines::bag::BagDiscipline;
use rholang::rust::interpreter::spaces::disciplines::cell::CellDiscipline;
use rholang::rust::interpreter::spaces::disciplines::queue::QueueDiscipline;
// Disabled with `ProbeSet`, which `SetDiscipline` replaces (SET batch): only `ProbeSet` used it.
// use rholang::rust::interpreter::spaces::disciplines::scan::{self, ScanDiscipline};
use rholang::rust::interpreter::spaces::disciplines::set::SetDiscipline;
use rholang::rust::interpreter::spaces::errors::SpaceError;
use rholang::rust::interpreter::spaces::generic::{Discipline, GenericSpace, OuterStore};
// Disabled with `ProbeSet`, which `SetDiscipline` replaces (SET batch): only `ProbeSet` used them.
// use rholang::rust::interpreter::spaces::generic::{Cx, Ordered, Side};
use rholang::rust::interpreter::spaces::namespace::{Durability, NativeSpace, SID_LEN};
use rholang::rust::interpreter::spaces::outer::hashmap::HashMapOuter;
use rholang::rust::interpreter::spaces::record;
use rholang::rust::interpreter::spaces::types::qualifier::SpaceQualifier;
use rspace_plus_plus::rspace::hashing::stable_hash_provider;
use rspace_plus_plus::rspace::internal::{Datum, WaitingContinuation};
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::space_store_handle::{
    canonical_continuation_order, canonical_data_order,
};
// Disabled with `ProbeSet`, which `SetDiscipline` replaces (SET batch): only `ProbeSet` used it.
// use rspace_plus_plus::rspace::space_store_handle::metered_encode;
use rspace_plus_plus::rspace::trace::event::{Event, IOEvent, Produce};
// Disabled with `ProbeSet`, which `SetDiscipline` replaces (SET batch): only `ProbeSet` used it.
// use serde::Serialize;
use support::agent::{
    any, budget, config_of, create, creation, data_channel, env_over, exactly, int, key_of, named,
    run, string, ParRSpace,
};
use support::props::{check, pure_config, tokio_runtime};

/// The space of every script.
const SPACE: NativeSpace = NativeSpace {
    sid: [0x5e; SID_LEN],
    durability: Durability::Durable,
};

/// The longest script of a property.
const SCRIPT_STEPS: usize = 8;

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

// ── The test disciplines ────────────────────────────────────────────────────────────────────

// Replaced by `SetDiscipline` of the SET batch (P5 wave W2, second wave): the set stand-in
// below keyed a class by the bincode of the payload. The real set key is the F5 key of
// `spaces/disciplines/set.rs`, which sorts each element and frames its length, so the engine
// properties now run the real set discipline.
// /// A set stand-in for the engine properties: the scan driver of A.7 with a class key. The key
// /// is the bincode of the payload pars, so two equal payloads form one class, and the random
// /// state, the authority and the cost stack play no part (F5). The SET batch owns the real key.
// #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
// struct ProbeSet;
//
// impl Discipline for ProbeSet {
//     const NAME: &'static str = "probeset";
//     const USES_COUNTER: bool = false;
//     const SEND_PARAMS: &'static [&'static str] = &[];
//
//     /// The head of the bag. Only the head driver uses it, and the scan driver does not.
//     fn head_metered<T: Serialize>(
//         &self,
//         side: Side,
//         entries: &[Ordered<'_, T>],
//         meter: &ScanMeter,
//     ) -> Result<Option<usize>, SpaceError> {
//         BagDiscipline.head_metered(side, entries, meter)
//     }
//
//     /// The rule of the bag. Only the head driver uses it.
//     fn reaches_head(&self, side: Side, queued: usize) -> bool {
//         BagDiscipline.reaches_head(side, queued)
//     }
//
//     /// The rule of the bag. Only the head driver uses it.
//     fn precedes(
//         &self,
//         side: Side,
//         ord: i64,
//         head: i64,
//         meter: &ScanMeter,
//     ) -> Result<bool, SpaceError> {
//         BagDiscipline.precedes(side, ord, head, meter)
//     }
//
//     /// OFFER-SCAN.
//     fn offer(
//         &self,
//         cx: &mut Cx<'_, '_>,
//         op: ProduceOp,
//         ord: i64,
//     ) -> Result<Option<SpaceReissue>, SpaceError> {
//         scan::offer(self, cx, op, ord)
//     }
//
//     /// RECEIVE-SCAN, with RESTORE-SCAN after a peek fire.
//     fn receive(
//         &self,
//         cx: &mut Cx<'_, '_>,
//         op: ConsumeOp,
//         ord: i64,
//     ) -> Result<Option<SpaceReissue>, SpaceError> {
//         scan::receive(self, cx, op, ord)
//     }
//
//     /// RESTORE-SCAN outside a receive.
//     fn restore(
//         &self,
//         cx: &mut Cx<'_, '_>,
//         record: ListParWithRandom,
//     ) -> Result<Option<SpaceReissue>, SpaceError> {
//         scan::restore(self, cx, record)
//     }
// }
//
// impl ScanDiscipline for ProbeSet {
//     /// The metered bincode of the payload pars.
//     fn class_key(&self, payload: &[Par], meter: &ScanMeter) -> Result<Option<Vec<u8>>, SpaceError> {
//         Ok(Some(metered_encode(payload, meter)?))
//     }
// }

/// A family under test.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Family {
    /// The head driver of S1.
    Queue,
    /// The scan driver without classes.
    Bag,
    /// The head driver with a capacity of one entry on each side.
    Cell,
    /// The scan driver with classes.
    Set,
}

// ── The injected matchers ───────────────────────────────────────────────────────────────────

/// The matcher that the host store injects into the space agents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Injected {
    /// The rholang matcher of the host.
    Real,
    /// A matcher that binds no payload.
    Refusing,
    /// The legacy WildcardMatch: every pattern binds every payload (D6).
    Wildcard,
}

/// A matcher that binds no payload and checks guards as the host does.
struct RefusingMatcher;

impl Match<BindPattern, ListParWithRandom, TaggedContinuation> for RefusingMatcher {
    fn get(&self, _pattern: &BindPattern, _data: &ListParWithRandom) -> Option<ListParWithRandom> {
        None
    }

    fn check_commit(
        &self,
        continuation: &TaggedContinuation,
        matched: &[ListParWithRandom],
    ) -> bool {
        Matcher.check_commit(continuation, matched)
    }

    fn commit_may_veto(&self, continuation: &TaggedContinuation) -> bool {
        Matcher.commit_may_veto(continuation)
    }

    fn check_commit_by_reference(
        &self,
        continuation: &TaggedContinuation,
        matched: &[&ListParWithRandom],
    ) -> bool {
        Matcher.check_commit_by_reference(continuation, matched)
    }
}

/// The legacy WildcardMatch: every pattern binds every payload as it is, and the guards are
/// checked as the host checks them.
struct WildcardMatcher;

impl Match<BindPattern, ListParWithRandom, TaggedContinuation> for WildcardMatcher {
    fn get(&self, _pattern: &BindPattern, data: &ListParWithRandom) -> Option<ListParWithRandom> {
        Some(data.clone())
    }

    fn check_commit(
        &self,
        continuation: &TaggedContinuation,
        matched: &[ListParWithRandom],
    ) -> bool {
        Matcher.check_commit(continuation, matched)
    }

    fn commit_may_veto(&self, continuation: &TaggedContinuation) -> bool {
        Matcher.commit_may_veto(continuation)
    }

    fn check_commit_by_reference(
        &self,
        continuation: &TaggedContinuation,
        matched: &[&ListParWithRandom],
    ) -> bool {
        Matcher.check_commit_by_reference(continuation, matched)
    }
}

/// A play space over a fresh in-memory store with the matcher `injected`.
async fn host_with(injected: Injected) -> ParRSpace {
    let mut stores = InMemoryStoreManager::new();
    let matcher: Arc<Box<dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>>> =
        match injected {
            Injected::Real => Arc::new(Box::new(Matcher)),
            Injected::Refusing => Arc::new(Box::new(RefusingMatcher)),
            Injected::Wildcard => Arc::new(Box::new(WildcardMatcher)),
        };
    ParRSpace::create(
        stores.r_space_stores().await.expect("in-memory stores"),
        matcher,
    )
    .expect("an in-memory space")
}

// ── Scripts ─────────────────────────────────────────────────────────────────────────────────

/// The pattern and the guard of a receive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Wanted {
    /// The pattern of the integer 1, with no guard.
    One,
    /// The pattern of the integer 2, with no guard.
    Two,
    /// The pattern of any value, with no guard.
    Any,
    /// The pattern of any value `x`, with the guard `x == 2`.
    AnyTwo,
}

impl Wanted {
    /// The pattern of the receive.
    fn pattern(self) -> BindPattern {
        match self {
            Wanted::One => exactly(1),
            Wanted::Two => exactly(2),
            Wanted::Any | Wanted::AnyTwo => any(),
        }
    }

    /// The where guard of the receive.
    fn guard(self) -> Option<Par> {
        match self {
            Wanted::AnyTwo => Some(bound_equals(2)),
            Wanted::One | Wanted::Two | Wanted::Any => None,
        }
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

/// One step of a script.
#[derive(Clone, Copy, Debug)]
enum Step {
    /// A send of the integer `value` (persistent: `!!`).
    Send { value: i64, persistent: bool },
    /// A receive (persistent: `<=`, peek: `<<-`). A receive never both persists and peeks.
    Receive {
        wanted: Wanted,
        persistent: bool,
        peek: bool,
    },
    /// The next round of the oldest re-issued operation, when one is in flight.
    Reenter,
}

/// A random step.
fn step() -> impl Strategy<Value = Step> {
    let wanted = prop_oneof![
        Just(Wanted::One),
        Just(Wanted::Two),
        Just(Wanted::Any),
        Just(Wanted::AnyTwo),
    ];
    prop_oneof![
        3 => (1i64..=2, prop::bool::weighted(0.25))
            .prop_map(|(value, persistent)| Step::Send { value, persistent }),
        3 => (wanted, prop::bool::weighted(0.25), prop::bool::weighted(0.25)).prop_map(
            |(wanted, persistent, peek)| Step::Receive {
                wanted,
                persistent: persistent && !peek,
                peek,
            }
        ),
        1 => Just(Step::Reenter),
    ]
}

/// A random script.
fn script() -> impl Strategy<Value = Vec<Step>> { vec(step(), 1..=SCRIPT_STEPS) }

/// A random receive that neither persists nor peeks.
fn plain_receive() -> impl Strategy<Value = Step> {
    prop_oneof![
        Just(Wanted::One),
        Just(Wanted::Two),
        Just(Wanted::Any),
        Just(Wanted::AnyTwo),
    ]
    .prop_map(|wanted| Step::Receive {
        wanted,
        persistent: false,
        peek: false,
    })
}

/// A random script that starts with one to three plain receives, so that guarded and unguarded
/// waiters often wait together before the first send.
fn waiting_then_script() -> impl Strategy<Value = Vec<Step>> {
    (vec(plain_receive(), 1..=3), script()).prop_map(|(mut waiting, rest)| {
        waiting.extend(rest);
        waiting
    })
}

/// The cost authority of the operation of step `index`.
fn region(index: usize) -> CostAuthority {
    let byte = u8::try_from(index % 250).expect("a small index") + 1;
    CostAuthority {
        regions: vec![CostRegion {
            instance_id: vec![byte; 32],
            signature: Some(CostSignature {
                value: Some(CostSignatureValue::Ground(vec![byte])),
            }),
        }],
    }
}

/// The token of the datum of step `index`: its random state.
fn token(index: usize) -> Vec<u8> {
    Blake2b512Random::create_from_bytes(format!("rrs engine datum {index}").as_bytes()).to_bytes()
}

/// The body name of the receive of step `index`.
fn receiver(index: usize) -> String { format!("k{index}") }

/// The send of step `index` on `channel`. The datum carries the authority of the send, as
/// `eval_send` stores it.
fn send_op(channel: &Par, index: usize, value: i64, persistent: bool) -> ProduceOp {
    ProduceOp {
        space: SPACE,
        key: key_of(channel),
        channel: channel.clone(),
        data: ListParWithRandom {
            pars: vec![int(value)],
            random_state: token(index),
            cost_authority: Some(region(index)),
            cost_stack: None,
        },
        persistent,
        params: BTreeMap::new(),
        authority: region(index),
        reissued_ord: None,
    }
}

/// The receive of step `index` on `channel`, with its own introduction authority.
fn receive_op(
    channel: &Par,
    index: usize,
    wanted: Wanted,
    persistent: bool,
    peek: bool,
) -> ConsumeOp {
    let body = match named(&receiver(index), None).tagged_cont {
        Some(TaggedCont::ParBody(body)) => body,
        Some(TaggedCont::ScalaBodyRef(_)) | None => ParWithRandom::default(),
    };
    ConsumeOp {
        space: SPACE,
        key: key_of(channel),
        channel: channel.clone(),
        pattern: wanted.pattern(),
        body,
        persistent,
        peek,
        guard: wanted.guard(),
        query: None,
        authority: CostAuthority::default(),
        introduction_authority: region(index),
        reissued_ord: None,
    }
}

/// What a script knows about a receive: its pattern, its guard and its flags.
#[derive(Clone, Debug)]
struct Receiver {
    pattern: BindPattern,
    continuation: TaggedContinuation,
    peek: bool,
}

/// What a script knows about a datum: its value, its persistence and its authority.
#[derive(Clone, Debug)]
struct Sent {
    value: i64,
    persistent: bool,
    authority: CostAuthority,
}

// ── Observations ────────────────────────────────────────────────────────────────────────────

/// A stored record, decoded.
#[derive(Clone, Debug)]
struct Rec {
    raw: Datum<ListParWithRandom>,
    token: Vec<u8>,
    value: i64,
    payload: ListParWithRandom,
}

/// A parked waiter, decoded.
#[derive(Clone, Debug)]
struct Wtr {
    raw: WaitingContinuation<BindPattern, TaggedContinuation>,
    name: String,
    pattern: BindPattern,
    continuation: TaggedContinuation,
    persistent: bool,
}

/// The collection in the store.
#[derive(Clone, Debug)]
struct Snapshot {
    records: Vec<Rec>,
    waiters: Vec<Wtr>,
}

impl Snapshot {
    /// The tokens of the stored records, with their multiplicities.
    fn tokens(&self) -> BTreeMap<Vec<u8>, usize> {
        let mut tokens = BTreeMap::new();
        for record in &self.records {
            *tokens.entry(record.token.clone()).or_insert(0) += 1;
        }
        tokens
    }

    /// The stored record with the token `token`.
    fn record(&self, token: &[u8]) -> Option<&Rec> {
        self.records.iter().find(|record| record.token == token)
    }

    /// The parked waiter with the body name `name`.
    fn waiter(&self, name: &str) -> Option<&Wtr> {
        self.waiters.iter().find(|waiter| waiter.name == name)
    }
}

/// One result of a fire, decoded.
#[derive(Clone, Debug)]
struct Bound {
    token: Vec<u8>,
    value: i64,
    persistent: bool,
    matched: ListParWithRandom,
    payload: ListParWithRandom,
}

/// One fire of an outcome, decoded: the body name of its continuation and its results.
#[derive(Clone, Debug)]
struct Fired {
    name: String,
    results: Vec<Bound>,
}

/// One operation of a script and what it did.
struct Observation {
    /// The operation.
    op: SpaceOp,
    /// The collection before the operation.
    before: Snapshot,
    /// The collection after the operation.
    after: Snapshot,
    /// The outcome or the error.
    result: Result<SpaceOutcome, SpaceError>,
    /// The fires of the outcome, decoded.
    fires: Vec<Fired>,
    /// The events of the operation.
    events: Vec<Event>,
    /// The authority that each produce on the data channel registered.
    registered: Vec<(Produce, Option<CostAuthority>)>,
    /// The tokens of the re-issued produces that are in flight after the operation.
    in_flight: BTreeSet<Vec<u8>>,
    /// The counter values of the collection after the operation.
    counter: Vec<i64>,
}

/// The run of one script.
struct Trace {
    family: Family,
    observations: Vec<Observation>,
    receivers: BTreeMap<String, Receiver>,
    sent: BTreeMap<Vec<u8>, Sent>,
}

/// The integer of a payload of one integer.
fn value_of(payload: &ListParWithRandom) -> i64 {
    match payload.pars.as_slice() {
        [Par { exprs, .. }] => match exprs.as_slice() {
            [Expr {
                expr_instance: Some(ExprInstance::GInt(value)),
            }] => *value,
            _ => panic!("a payload of the scripts holds one integer"),
        },
        _ => panic!("a payload of the scripts holds one integer"),
    }
}

/// The body name of a continuation that `named` made.
fn name_of(continuation: &TaggedContinuation) -> String { support::agent::body_name(continuation) }

/// Reads the collection on the data channel `data`.
async fn snapshot(host: &ParRSpace, data: &Par) -> Snapshot {
    let records = host
        .get_data(data)
        .await
        .into_iter()
        .map(|raw| {
            let unwrapped = record::unwrap_datum(&raw.a).expect("a datum record");
            Rec {
                token: unwrapped.datum.random_state.clone(),
                value: value_of(&unwrapped.datum),
                payload: unwrapped.datum,
                raw,
            }
        })
        .collect();
    let waiters = host
        .get_waiting_continuations(vec![data.clone()])
        .await
        .into_iter()
        .map(|raw| {
            let continuation = record::unwrap_continuation(&raw.continuation)
                .expect("a continuation record")
                .continuation;
            Wtr {
                name: name_of(&continuation),
                pattern: raw.patterns.first().cloned().expect("one bind"),
                persistent: raw.persist,
                continuation,
                raw,
            }
        })
        .collect();
    Snapshot { records, waiters }
}

/// The counter values on `channel`.
async fn counter_values(host: &ParRSpace, channel: &Par) -> Vec<i64> {
    host.get_data(channel)
        .await
        .iter()
        .map(|datum| record::counter_value(&datum.a).expect("a counter record"))
        .collect()
}

/// The fires of `outcome`, decoded.
fn decode_fires(outcome: &SpaceOutcome) -> Vec<Fired> {
    outcome
        .fires
        .iter()
        .map(|fire| Fired {
            name: name_of(&fire.continuation),
            results: fire
                .results
                .iter()
                .map(|result| {
                    let removed = record::unwrap_datum(&result.removed_datum)
                        .expect("a fired record")
                        .datum;
                    Bound {
                        token: removed.random_state.clone(),
                        value: value_of(&removed),
                        persistent: result.persistent,
                        matched: result.matched_datum.clone(),
                        payload: removed,
                    }
                })
                .collect(),
        })
        .collect()
}

/// The configuration of a run.
#[derive(Clone, Copy, Debug)]
struct Setup {
    family: Family,
    injected: Injected,
}

/// Runs `script` with the agent of `family` over a fresh host store with the matcher
/// `injected`, and observes every operation.
async fn run_script(setup: Setup, script: &[Step]) -> Trace {
    match setup.family {
        Family::Queue => run_with::<QueueDiscipline>(setup, script).await,
        Family::Bag => run_with::<BagDiscipline>(setup, script).await,
        Family::Cell => run_with::<CellDiscipline>(setup, script).await,
        Family::Set => run_with::<SetDiscipline>(setup, script).await,
    }
}

/// The SpaceOp of `step` at the index `index`, or the next re-issue for a re-entry.
fn op_of(
    step: Step,
    index: usize,
    channel: &Par,
    pending: &mut VecDeque<SpaceOp>,
    receivers: &mut BTreeMap<String, Receiver>,
    sent: &mut BTreeMap<Vec<u8>, Sent>,
) -> Option<SpaceOp> {
    match step {
        Step::Send { value, persistent } => {
            sent.insert(token(index), Sent {
                value,
                persistent,
                authority: region(index),
            });
            Some(SpaceOp::Produce(send_op(channel, index, value, persistent)))
        }
        Step::Receive {
            wanted,
            persistent,
            peek,
        } => {
            receivers.insert(receiver(index), Receiver {
                pattern: wanted.pattern(),
                continuation: named(&receiver(index), wanted.guard()),
                peek,
            });
            Some(SpaceOp::Consume(receive_op(
                channel, index, wanted, persistent, peek,
            )))
        }
        Step::Reenter => pending.pop_front(),
    }
}

/// [`run_script`] for the discipline `D`.
async fn run_with<D: Discipline>(setup: Setup, script: &[Step]) -> Trace {
    let host = host_with(setup.injected).await;
    let budget = budget();
    let _scope = budget.enter_comm_accounting_scope();
    let env = env_over(host.clone(), &budget);
    let agent = GenericSpace::<HashMapOuter, D>::default();
    let config = config_of(SPACE, HashMapOuter::NAME, D::NAME, SpaceQualifier::Default);
    create(
        &agent,
        &env,
        creation(SPACE, config, SpaceFactoryUrn::HASHMAP_QUEUE),
    )
    .await;
    host.take_event_log().await;
    let channel = string("rrs-engine");
    let data = data_channel(SPACE, &channel);
    let counter = support::agent::counter_channel(SPACE, &channel);
    let data_hash = stable_hash_provider::hash(&data);
    let mut pending = VecDeque::new();
    let mut receivers = BTreeMap::new();
    let mut sent = BTreeMap::new();
    let mut observations = Vec::with_capacity(script.len());
    for (index, step) in script.iter().enumerate() {
        let Some(op) = op_of(
            *step,
            index,
            &channel,
            &mut pending,
            &mut receivers,
            &mut sent,
        ) else {
            continue;
        };
        let before = snapshot(&host, &data).await;
        let result = run(&agent, &env, op.clone()).await;
        let events = host.take_event_log().await;
        let after = snapshot(&host, &data).await;
        let fires = match &result {
            Ok(outcome) => decode_fires(outcome),
            Err(_) => Vec::new(),
        };
        if let Ok(SpaceOutcome {
            reissue: Some(reissue),
            ..
        }) = &result
        {
            pending.push_back(reissue.op.clone());
        }
        let registered = events
            .iter()
            .filter_map(|event| match event {
                Event::IoEvent(IOEvent::Produce(produce)) if produce.channel_hash == data_hash => {
                    let authority = budget
                        .introduction_authority(
                            produce_introduction_identity(produce),
                            AuthorityByteEventKind::ProduceIntroduction,
                        )
                        .ok();
                    Some((produce.clone(), authority))
                }
                _ => None,
            })
            .collect();
        let in_flight = pending
            .iter()
            .filter_map(|op| match op {
                SpaceOp::Produce(produce) => Some(produce.data.random_state.clone()),
                SpaceOp::Consume(_) | SpaceOp::Settle(_) | SpaceOp::Join(_) => None,
            })
            .collect();
        observations.push(Observation {
            op,
            before,
            after,
            result,
            fires,
            events,
            registered,
            in_flight,
            counter: counter_values(&host, &counter).await,
        });
    }
    Trace {
        family: setup.family,
        observations,
        receivers,
        sent,
    }
}

/// Runs `script` for each family of `families` with the real matcher, and checks each trace.
fn for_families(
    families: &[Family],
    script: &[Step],
    property: impl Fn(&Trace) -> Result<(), TestCaseError>,
) -> Result<(), TestCaseError> {
    for &family in families {
        let trace = tokio_runtime().block_on(run_script(
            Setup {
                family,
                injected: Injected::Real,
            },
            script,
        ));
        property(&trace).map_err(|error| TestCaseError::fail(format!("{family:?}: {error}")))?;
    }
    Ok(())
}

// ── The oracle of the matcher ───────────────────────────────────────────────────────────────

/// The spatial match of the host matcher.
fn binds(pattern: &BindPattern, payload: &ListParWithRandom) -> Option<ListParWithRandom> {
    Matcher.get(pattern, payload)
}

/// The guard of the host matcher.
fn admits(continuation: &TaggedContinuation, matched: &ListParWithRandom) -> bool {
    Matcher.check_commit(continuation, std::slice::from_ref(matched))
}

/// True when `waiter` binds `payload` spatially and its guard admits the binding.
fn takes(waiter: &Wtr, payload: &ListParWithRandom) -> bool {
    binds(&waiter.pattern, payload).is_some_and(|matched| admits(&waiter.continuation, &matched))
}

/// The receive of `op`, when it is one.
fn receive_name(op: &SpaceOp) -> Option<String> {
    match op {
        SpaceOp::Consume(ConsumeOp { body, .. }) | SpaceOp::Join(JoinOp { body, .. }) => {
            body.body.as_ref().map(|_| {
                name_of(&TaggedContinuation {
                    tagged_cont: Some(TaggedCont::ParBody(body.clone())),
                    guard: None,
                    cost_authority: None,
                })
            })
        }
        SpaceOp::Produce(_) | SpaceOp::Settle(_) => None,
    }
}

/// The token of the datum of `op`, when it is a produce.
fn produce_token(op: &SpaceOp) -> Option<Vec<u8>> {
    match op {
        SpaceOp::Produce(produce) => Some(produce.data.random_state.clone()),
        SpaceOp::Consume(_) | SpaceOp::Settle(_) | SpaceOp::Join(_) => None,
    }
}

/// True when the fire `fire` of the observation `obs` is the fire of the receive of the
/// operation (a consume-side fire).
fn consume_side(obs: &Observation, fire: &Fired) -> bool {
    receive_name(&obs.op).is_some_and(|name| name == fire.name)
}

/// The receiver of a fire.
fn receiver_of<'t>(trace: &'t Trace, fire: &Fired) -> &'t Receiver {
    trace
        .receivers
        .get(&fire.name)
        .expect("every fired continuation is a receive of the script")
}

/// The tokens that a fire spends: the non-persistent results of a fire whose continuation does
/// not peek.
fn spent_by(trace: &Trace, fire: &Fired) -> Vec<Vec<u8>> {
    match receiver_of(trace, fire).peek {
        true => Vec::new(),
        false => fire
            .results
            .iter()
            .filter(|result| !result.persistent)
            .map(|result| result.token.clone())
            .collect(),
    }
}

/// The values of the fires of an operation whose continuation does not peek.
fn non_peek_values(trace: &Trace, obs: &Observation) -> BTreeSet<i64> {
    obs.fires
        .iter()
        .filter(|fire| !receiver_of(trace, fire).peek)
        .flat_map(|fire| fire.results.iter().map(|result| result.value))
        .collect()
}

// ── F17, F18, F19: Match.v ──────────────────────────────────────────────────────────────────

/// F17 (`space_match_is_injected_match`): the engine matches with the injected matcher of its
/// host store. Under the real matcher every fire binds what the host matcher binds. Under a
/// matcher that binds nothing, no operation fires: a bag or a set stores every send and parks
/// every receive. The counter take of a queue or a cell binds its counter record with the same
/// matcher, so each of their operations fails before it stages anything.
#[test]
fn prop_engine_match_is_the_injected_matcher() {
    check(pure_config(), script(), |script| {
        for family in [Family::Queue, Family::Bag, Family::Cell, Family::Set] {
            let real = tokio_runtime().block_on(run_script(
                Setup {
                    family,
                    injected: Injected::Real,
                },
                &script,
            ));
            for obs in &real.observations {
                for fire in &obs.fires {
                    let receiver = receiver_of(&real, fire);
                    for result in fire.results.iter().take(1) {
                        prop_assert_eq!(
                            binds(&receiver.pattern, &result.payload),
                            Some(result.matched.clone()),
                            "{:?}: a fire binds what the injected matcher binds",
                            family
                        );
                    }
                }
            }
            let refusing = tokio_runtime().block_on(run_script(
                Setup {
                    family,
                    injected: Injected::Refusing,
                },
                &script,
            ));
            for obs in &refusing.observations {
                prop_assert!(
                    obs.fires.is_empty(),
                    "{:?}: a refusing matcher fires nothing",
                    family
                );
                match (family, &obs.result) {
                    // The counter take binds the counter record with the injected matcher too, so
                    // a family with a counter fails before it stages anything.
                    (Family::Queue | Family::Cell, result) => {
                        prop_assert_eq!(
                            result.as_ref().err(),
                            Some(&SpaceError::Inconsistent(
                                "a counter record does not match the counter pattern"
                            ))
                        );
                        prop_assert!(obs.events.is_empty(), "the failed bump stages nothing");
                    }
                    (Family::Bag | Family::Set, Ok(outcome)) => {
                        prop_assert!(outcome.reissue.is_none());
                    }
                    (Family::Bag | Family::Set, Err(error)) => {
                        prop_assert!(false, "{:?}: a scan family fails: {:?}", family, error);
                    }
                }
            }
        }
        Ok(())
    });
}

/// The choices of a send under decision 37 for a scan family: each fire takes the first
/// remaining waiter, in canonical order, that binds the datum and admits it, and a stored datum
/// has no such waiter.
fn check_offer_choices(
    obs: &Observation,
    payload: &ListParWithRandom,
    skipped: &Hits,
) -> Result<(), TestCaseError> {
    let raw: Vec<_> = obs
        .before
        .waiters
        .iter()
        .map(|waiter| waiter.raw.clone())
        .collect();
    let order = canonical_continuation_order(&raw);
    let mut remaining: Vec<&Wtr> = order
        .iter()
        .map(|&index| &obs.before.waiters[index])
        .collect();
    let token = payload.random_state.clone();
    for fire in obs
        .fires
        .iter()
        .filter(|fire| fire.results.iter().any(|r| r.token == token))
    {
        let expected = remaining.iter().position(|waiter| takes(waiter, payload));
        prop_assert!(expected.is_some(), "a send fires only an admitting waiter");
        let expected = expected.expect("checked above");
        if remaining[..expected]
            .iter()
            .any(|waiter| binds(&waiter.pattern, payload).is_some())
        {
            skipped.hit();
        }
        prop_assert_eq!(
            &fire.name,
            &remaining[expected].name,
            "the first admitting waiter fires"
        );
        if !remaining[expected].persistent {
            remaining.remove(expected);
        }
    }
    if obs.after.record(&token).is_some() && obs.before.record(&token).is_none() {
        prop_assert!(
            !remaining.iter().any(|waiter| takes(waiter, payload)),
            "a datum is stored only when no waiter admits it"
        );
    }
    Ok(())
}

/// The choice of a receive under decision 37 for a scan family: the first record, in canonical
/// order, that the pattern binds, and one guard check on it.
fn check_receive_choice(
    trace: &Trace,
    obs: &Observation,
    name: &str,
    refused: &Hits,
) -> Result<(), TestCaseError> {
    let receiver = trace.receivers.get(name).expect("a receive of the script");
    let raw: Vec<_> = obs
        .before
        .records
        .iter()
        .map(|record| record.raw.clone())
        .collect();
    let order = canonical_data_order(&raw);
    let first = order
        .iter()
        .map(|&index| &obs.before.records[index])
        .find(|record| binds(&receiver.pattern, &record.payload).is_some());
    let own: Vec<&Fired> = obs.fires.iter().filter(|fire| fire.name == name).collect();
    match first {
        None => prop_assert!(own.is_empty(), "a receive with no spatial match parks"),
        Some(record) => {
            let matched = binds(&receiver.pattern, &record.payload).expect("checked above");
            match admits(&receiver.continuation, &matched) {
                false => {
                    prop_assert!(own.is_empty(), "a guard refusal of the first match parks");
                    refused.hit();
                }
                true => {
                    let fire = own.first();
                    prop_assert!(fire.is_some(), "the first spatial match fires");
                    let fire = fire.expect("checked above");
                    prop_assert_eq!(
                        &fire.results[0].token,
                        &record.token,
                        "the first spatial match in canonical order fires"
                    );
                }
            }
        }
    }
    Ok(())
}

/// F18 (`guard_once_greedy_like_head`): a scan family chooses as the host rule of decision 37
/// over the canonical order: a receive takes the first spatial match and checks the guard once,
/// and a send fires the first waiter that binds and admits its datum. The cell checks its one
/// head, as the head driver does.
#[test]
fn prop_engine_guard_checked_once_as_the_host_checks_it() {
    let skipped = Hits::new();
    let refused = Hits::new();
    check(pure_config(), waiting_then_script(), |script| {
        for_families(
            &[Family::Bag, Family::Set, Family::Cell],
            &script,
            |trace| {
                for obs in &trace.observations {
                    if obs.result.is_err()
                        || (obs.before.records.len() > 1 && trace.family == Family::Cell)
                    {
                        continue;
                    }
                    match &obs.op {
                        SpaceOp::Produce(produce) if produce.reissued_ord.is_none() => {
                            let payload = produce.data.clone();
                            check_offer_choices(obs, &payload, &skipped)?;
                        }
                        SpaceOp::Consume(_) => {
                            let name = receive_name(&obs.op).expect("a receive");
                            check_receive_choice(trace, obs, &name, &refused)?;
                        }
                        SpaceOp::Produce(_) | SpaceOp::Settle(_) | SpaceOp::Join(_) => {}
                    }
                }
                Ok(())
            },
        )
    });
    skipped.assert_at_least(4, "a send fires a later waiter after a guard refusal");
    refused.assert_at_least(1, "a receive parks on a guard refusal of its first match");
}

/// F19 (`wildcard_matcher_degenerate`): under the legacy WildcardMatch a receive for one value
/// receives another value, and under the real matcher it parks.
#[test]
fn prop_engine_wildcard_matcher_breaks_pattern_enforcement() {
    let strategy = (1i64..=2, prop_oneof![
        Just(Family::Queue),
        Just(Family::Bag),
        Just(Family::Cell),
        Just(Family::Set)
    ]);
    check(pure_config(), strategy, |(sent, family)| {
        let wanted = match sent {
            1 => Wanted::Two,
            _ => Wanted::One,
        };
        let script = [
            Step::Send {
                value: sent,
                persistent: false,
            },
            Step::Receive {
                wanted,
                persistent: false,
                peek: false,
            },
        ];
        let wildcard = tokio_runtime().block_on(run_script(
            Setup {
                family,
                injected: Injected::Wildcard,
            },
            &script,
        ));
        let fires = &wildcard.observations[1].fires;
        prop_assert_eq!(fires.len(), 1, "the wildcard receive fires");
        prop_assert_eq!(
            fires[0].results[0].value,
            sent,
            "a pattern for one value receives another"
        );
        prop_assert!(binds(&wanted.pattern(), &fires[0].results[0].payload).is_none());
        let real = tokio_runtime().block_on(run_script(
            Setup {
                family,
                injected: Injected::Real,
            },
            &script,
        ));
        prop_assert!(
            real.observations[1].fires.is_empty(),
            "the real matcher parks"
        );
        prop_assert_eq!(real.observations[1].after.waiters.len(), 1);
        Ok(())
    });
}

// ── F20, F21, F22, Rule E, decision 43: GenericRSpace.v ─────────────────────────────────────

/// F20 (`consume_fires_only_matching`): every consume-side fire dispatches one stored record
/// that the pattern binds and the guard admits, and every member of a class group was stored.
#[test]
fn prop_engine_consume_fires_only_matching() {
    check(pure_config(), script(), |script| {
        for_families(
            &[Family::Queue, Family::Bag, Family::Cell, Family::Set],
            &script,
            |trace| {
                for obs in &trace.observations {
                    for fire in obs.fires.iter().filter(|fire| consume_side(obs, fire)) {
                        let receiver = receiver_of(trace, fire);
                        prop_assert_eq!(
                            fire.results.len(),
                            1,
                            "a group fire dispatches one result"
                        );
                        let result = &fire.results[0];
                        prop_assert!(
                            obs.before.record(&result.token).is_some(),
                            "a stored record"
                        );
                        let matched = binds(&receiver.pattern, &result.payload);
                        prop_assert_eq!(matched.as_ref(), Some(&result.matched));
                        prop_assert!(admits(&receiver.continuation, &result.matched));
                    }
                }
                Ok(())
            },
        )
    });
}

/// F21 (`produce_fires_only_matching`): every produce-side fire binds a stored waiter that
/// binds the datum and whose guard admits it.
#[test]
fn prop_engine_produce_fires_only_matching() {
    check(pure_config(), script(), |script| {
        for_families(
            &[Family::Queue, Family::Bag, Family::Cell, Family::Set],
            &script,
            |trace| {
                for obs in &trace.observations {
                    for fire in obs.fires.iter().filter(|fire| !consume_side(obs, fire)) {
                        let waiter = obs.before.waiter(&fire.name);
                        prop_assert!(
                            waiter.is_some(),
                            "a produce-side fire fires a parked waiter"
                        );
                        let waiter = waiter.expect("checked above");
                        for result in &fire.results {
                            prop_assert!(
                                takes(waiter, &result.payload),
                                "the waiter admits the datum"
                            );
                            let matched = binds(&waiter.pattern, &result.payload);
                            prop_assert_eq!(matched.as_ref(), Some(&result.matched));
                        }
                    }
                }
                Ok(())
            },
        )
    });
}

/// True when a parked waiter without a guard binds a stored record: the pending match that V1
/// excludes for a bag, a set and a cell.
fn guard_free_pending(snapshot: &Snapshot) -> bool {
    snapshot
        .waiters
        .iter()
        .filter(|waiter| waiter.continuation.guard.is_none())
        .any(|waiter| {
            snapshot
                .records
                .iter()
                .any(|record| binds(&waiter.pattern, &record.payload).is_some())
        })
}

/// F22 (`unordered_no_pending_match`) and V1 (`NoPendingMatch`): after every operation of a
/// bag, a set or a cell, no parked waiter without a guard binds a stored record.
#[test]
fn prop_engine_no_guard_free_pending_match() {
    let together = Hits::new();
    check(pure_config(), script(), |script| {
        for_families(
            &[Family::Bag, Family::Set, Family::Cell],
            &script,
            |trace| {
                for obs in &trace.observations {
                    if !obs.after.records.is_empty()
                        && obs
                            .after
                            .waiters
                            .iter()
                            .any(|w| w.continuation.guard.is_none())
                    {
                        together.hit();
                    }
                    prop_assert!(
                        !guard_free_pending(&obs.after),
                        "a waiter without a guard has no pending match"
                    );
                }
                Ok(())
            },
        )
    });
    together.assert_at_least(
        32,
        "a waiter without a guard and a record are stored together",
    );
}

/// The facts of one consume-side peek fire of a non-persistent record: the record is stored
/// again with its identity and its own authority, or the re-offer gives it to one waiter.
fn check_peek_restore(
    trace: &Trace,
    obs: &Observation,
    fire: &Fired,
    restored_hits: &Hits,
) -> Result<(), TestCaseError> {
    let result = &fire.results[0];
    if result.persistent {
        prop_assert!(
            obs.after.record(&result.token).is_some(),
            "a persistent record stays"
        );
        return Ok(());
    }
    let before = obs.before.record(&result.token).expect("a stored record");
    let delivered = obs
        .fires
        .iter()
        .filter(|other| !consume_side(obs, other))
        .any(|other| {
            !receiver_of(trace, other).peek && other.results.iter().any(|r| r.token == result.token)
        });
    match obs.after.record(&result.token) {
        Some(after) => {
            prop_assert!(
                !delivered,
                "a re-sent record is stored or delivered, not both"
            );
            prop_assert_eq!(&after.raw.a, &before.raw.a, "the same record bytes");
            prop_assert_eq!(&after.raw.source, &before.raw.source, "the same identity");
            prop_assert!(!after.raw.persist, "the second send is not persistent");
            restored_hits.hit();
            let own = trace
                .sent
                .get(&result.token)
                .map(|sent| sent.authority.clone());
            let restored = obs
                .registered
                .iter()
                .rev()
                .find(|(produce, _)| produce.hash == before.raw.source.hash);
            prop_assert!(restored.is_some(), "the re-store produces the record again");
            prop_assert_eq!(
                &restored.expect("checked above").1,
                &own,
                "its own authority"
            );
            if trace.family != Family::Queue {
                prop_assert!(
                    !obs.after
                        .waiters
                        .iter()
                        .any(|waiter| takes(waiter, &after.payload)),
                    "the second send stores the record only when no waiter admits it"
                );
            }
        }
        None => prop_assert!(
            delivered || obs.in_flight.contains(&result.token),
            "a peeked record is stored again, delivered by its second send, or in flight"
        ),
    }
    Ok(())
}

/// Rule E (`peek_restore_equals_host_reproduce`): a consume-side peek of a non-persistent record
/// is followed by the host's second send of that record: the same bytes and identity, not
/// persistent, with the record's own authority, offered to the waiters before it is stored. A
/// peek bumps the counter of an ordered collection once, for its operation only.
#[test]
fn prop_engine_peek_restore_equals_host_reproduce() {
    let restored = Hits::new();
    check(pure_config(), script(), |script| {
        for_families(
            &[Family::Queue, Family::Bag, Family::Cell, Family::Set],
            &script,
            |trace| {
                let mut last_counter = Vec::new();
                for obs in &trace.observations {
                    let peek = matches!(&obs.op, SpaceOp::Consume(consume) if consume.peek);
                    for fire in obs.fires.iter().filter(|fire| consume_side(obs, fire)) {
                        if peek {
                            check_peek_restore(trace, obs, fire, &restored)?;
                        }
                    }
                    if peek
                        && obs.result.is_ok()
                        && matches!(trace.family, Family::Queue | Family::Cell)
                    {
                        let previous = last_counter.first().copied().unwrap_or(0);
                        prop_assert_eq!(
                            obs.counter.first().copied(),
                            Some(previous + 1),
                            "a peek and its re-store bump the counter once"
                        );
                    }
                    if obs.result.is_ok() {
                        last_counter = obs.counter.clone();
                    }
                }
                Ok(())
            },
        )
    });
    restored.assert_at_least(12, "a peek stores its record again");
}

/// The produces of the data channel in the events of `obs`, in event order.
fn produce_hashes(obs: &Observation) -> Vec<Produce> {
    obs.registered
        .iter()
        .map(|(produce, _)| produce.clone())
        .collect()
}

/// Decision 43 (`scan_reissue_identity_stable`): a collection without a counter re-issues an
/// element with no carried order, and every round of the element stages the record and the
/// identity of its first round. A re-issued order other than 0 is `Inconsistent`.
#[test]
fn prop_engine_scan_reissue_keeps_identity() {
    let strategy = (1i64..=2, 1usize..=3, prop_oneof![
        Just(Family::Bag),
        Just(Family::Set)
    ]);
    check(pure_config(), strategy, |(value, waiters, family)| {
        // A persistent send meets a persistent waiter: a divergent pair, then re-entries.
        let mut script = vec![Step::Receive {
            wanted: Wanted::Any,
            persistent: true,
            peek: false,
        }];
        for _ in 0..waiters {
            script.push(Step::Receive {
                wanted: Wanted::Any,
                persistent: false,
                peek: true,
            });
        }
        script.push(Step::Send {
            value,
            persistent: true,
        });
        script.extend([Step::Reenter, Step::Reenter]);
        let trace = tokio_runtime().block_on(run_script(
            Setup {
                family,
                injected: Injected::Real,
            },
            &script,
        ));
        let rounds: Vec<&Observation> = trace
            .observations
            .iter()
            .filter(|obs| matches!(&obs.op, SpaceOp::Produce(_)))
            .collect();
        prop_assert!(rounds.len() >= 2, "the divergent pair re-issues the send");
        let first = produce_hashes(rounds[0]);
        prop_assert!(!first.is_empty());
        for round in &rounds {
            let SpaceOp::Produce(produce) = &round.op else {
                unreachable!("filtered above")
            };
            prop_assert_eq!(produce.reissued_ord, None, "a re-issue carries no order");
            prop_assert_eq!(
                &produce_hashes(round)[0],
                &first[0],
                "every round has one identity"
            );
            if let Ok(SpaceOutcome {
                reissue: Some(reissue),
                ..
            }) = &round.result
            {
                prop_assert!(
                    matches!(&reissue.op, SpaceOp::Produce(op) if op.reissued_ord.is_none())
                );
            }
        }
        // A re-issued order other than 0 is Inconsistent in a collection without a counter.
        let rejected = tokio_runtime().block_on(async {
            let host = host_with(Injected::Real).await;
            let budget = budget();
            let env = env_over(host.clone(), &budget);
            let channel = string("rrs-engine");
            let outcome = match family {
                Family::Bag => {
                    reissued_order_rejected::<BagDiscipline>(&host, &env, &channel, value).await
                }
                _ => reissued_order_rejected::<SetDiscipline>(&host, &env, &channel, value).await,
            };
            (outcome, host.take_event_log().await)
        });
        prop_assert_eq!(
            rejected.0,
            Err(SpaceError::Inconsistent(
                "a re-issued order of a collection without a counter is not 0"
            ))
        );
        prop_assert!(rejected.1.is_empty(), "a rejected re-issue stages nothing");
        Ok(())
    });
}

/// Creates a space of `D` and runs a send with the re-issued order 3, which a collection without
/// a counter never gives.
async fn reissued_order_rejected<D: Discipline>(
    host: &ParRSpace,
    env: &SpaceEnv,
    channel: &Par,
    value: i64,
) -> Result<SpaceOutcome, SpaceError> {
    let agent = GenericSpace::<HashMapOuter, D>::default();
    let config = config_of(SPACE, HashMapOuter::NAME, D::NAME, SpaceQualifier::Default);
    create(
        &agent,
        env,
        creation(SPACE, config, SpaceFactoryUrn::HASHMAP_QUEUE),
    )
    .await;
    host.take_event_log().await;
    let op = ProduceOp {
        reissued_ord: Some(3),
        ..send_op(channel, 0, value, true)
    };
    run(&agent, env, SpaceOp::Produce(op)).await
}

// ── V2 to V8: GenericRSpace.tla ─────────────────────────────────────────────────────────────

/// V2 (`PatternEnforcement`): every fire binds a value that its pattern binds and that its
/// guard admits, on both sides.
#[test]
fn prop_engine_fires_enforce_patterns() {
    check(pure_config(), script(), |script| {
        for_families(
            &[Family::Queue, Family::Bag, Family::Cell, Family::Set],
            &script,
            |trace| {
                for obs in &trace.observations {
                    for fire in &obs.fires {
                        let receiver = receiver_of(trace, fire);
                        for result in fire.results.iter().take(1) {
                            let matched = binds(&receiver.pattern, &result.payload);
                            prop_assert!(matched.is_some(), "the pattern binds the value");
                            prop_assert!(admits(&receiver.continuation, &result.matched));
                        }
                    }
                }
                Ok(())
            },
        )
    });
}

/// The fate of each datum after an operation: stored, spent, or in flight.
fn check_fates(
    trace: &Trace,
    obs: &Observation,
    spent: &BTreeSet<Vec<u8>>,
) -> Result<(), TestCaseError> {
    let stored = obs.after.tokens();
    for (token, count) in &stored {
        prop_assert_eq!(*count, 1, "a record is never stored twice");
        prop_assert!(!spent.contains(token), "a spent datum is never stored");
        prop_assert!(
            !obs.in_flight.contains(token),
            "a stored datum is not in flight"
        );
    }
    let mut live: BTreeSet<Vec<u8>> = obs.before.tokens().into_keys().collect();
    live.extend(produce_token(&obs.op));
    let retired = match trace.family {
        Family::Set => non_peek_values(trace, obs),
        Family::Queue | Family::Bag | Family::Cell => BTreeSet::new(),
    };
    for token in live {
        let kept = stored.contains_key(&token) || obs.in_flight.contains(&token);
        let gone = spent.contains(&token);
        let class_retired = trace
            .sent
            .get(&token)
            .is_some_and(|sent| !sent.persistent && retired.contains(&sent.value));
        prop_assert!(
            kept || gone || class_retired,
            "a datum is stored, spent, retired with its class, or in flight"
        );
        prop_assert!(!(kept && gone), "a datum is never both kept and spent");
    }
    Ok(())
}

/// V3 (`Conservation`): after every operation each datum is stored, spent by a fire that does
/// not peek, retired with its class (set), or in flight, and never two of them. A persistent
/// datum is never spent.
#[test]
fn prop_engine_conserves_records() {
    let flying = Hits::new();
    check(pure_config(), script(), |script| {
        for_families(
            &[Family::Queue, Family::Bag, Family::Cell, Family::Set],
            &script,
            |trace| {
                let mut spent = BTreeSet::new();
                for obs in &trace.observations {
                    if !obs.in_flight.is_empty() {
                        flying.hit();
                    }
                    if obs.result.is_err() {
                        prop_assert_eq!(obs.before.tokens(), obs.after.tokens());
                        continue;
                    }
                    for fire in &obs.fires {
                        for token in spent_by(trace, fire) {
                            prop_assert!(
                                trace.sent.get(&token).is_some_and(|sent| !sent.persistent),
                                "only a datum that is not persistent is spent"
                            );
                            spent.insert(token);
                        }
                    }
                    if trace.family == Family::Set {
                        let values = non_peek_values(trace, obs);
                        for record in &obs.before.records {
                            if !record.raw.persist
                                && values.contains(&record.value)
                                && obs.after.record(&record.token).is_none()
                            {
                                spent.insert(record.token.clone());
                            }
                        }
                    }
                    check_fates(trace, obs, &spent)?;
                }
                Ok(())
            },
        )
    });
    flying.assert_at_least(2, "a datum is in flight between two rounds");
}

/// V4 (`NoDoubleSpend`): a datum that is not persistent is spent by at most one fire.
#[test]
fn prop_engine_spends_each_record_once() {
    check(pure_config(), script(), |script| {
        for_families(
            &[Family::Queue, Family::Bag, Family::Cell, Family::Set],
            &script,
            |trace| {
                let mut spent = BTreeSet::new();
                for obs in &trace.observations {
                    for fire in &obs.fires {
                        for token in spent_by(trace, fire) {
                            prop_assert!(spent.insert(token), "a datum is spent at most once");
                        }
                    }
                }
                Ok(())
            },
        )
    });
}

/// V5 (`AtomicReservation`): the stored records that a fire binds were stored before the
/// operation, or they are its incoming datum, and a fire that does not peek leaves none of its
/// non-persistent records stored.
#[test]
fn prop_engine_fire_removes_present_records_atomically() {
    check(pure_config(), script(), |script| {
        for_families(
            &[Family::Queue, Family::Bag, Family::Cell, Family::Set],
            &script,
            |trace| {
                for obs in &trace.observations {
                    let incoming = produce_token(&obs.op);
                    for fire in &obs.fires {
                        for result in &fire.results {
                            let present = obs.before.record(&result.token).is_some()
                                || incoming.as_ref() == Some(&result.token);
                            prop_assert!(present, "a fire binds a present record");
                        }
                        for token in spent_by(trace, fire) {
                            prop_assert!(
                                obs.after.record(&token).is_none(),
                                "a spent record is absent after its fire"
                            );
                        }
                    }
                }
                Ok(())
            },
        )
    });
}

/// V6 (`SetClassRetired`): after a fire of class v that does not peek, no non-persistent record
/// of value v remains in the set.
#[test]
fn prop_engine_set_class_retired() {
    let classes = Hits::new();
    check(pure_config(), script(), |script| {
        for_families(&[Family::Set], &script, |trace| {
            for obs in &trace.observations {
                for value in non_peek_values(trace, obs) {
                    let members = obs
                        .before
                        .records
                        .iter()
                        .filter(|record| record.value == value && !record.raw.persist)
                        .count();
                    if members >= 2 {
                        classes.hit();
                    }
                    prop_assert!(
                        !obs.after
                            .records
                            .iter()
                            .any(|record| record.value == value && !record.raw.persist),
                        "the class of a fire that does not peek leaves the set"
                    );
                }
            }
            Ok(())
        })
    });
    classes.assert_at_least(3, "a class of two or more members leaves at once");
}

/// V7 (`CellCapacity`): a cell holds at most one record and one waiter, and an operation that
/// fails with `CellOccupied` changes nothing.
#[test]
fn prop_engine_cell_capacity() {
    let aborts = Hits::new();
    check(pure_config(), script(), |script| {
        for_families(&[Family::Cell], &script, |trace| {
            let mut counter = Vec::new();
            for obs in &trace.observations {
                prop_assert!(obs.after.records.len() <= 1);
                prop_assert!(obs.after.waiters.len() <= 1);
                if let Err(error) = &obs.result {
                    aborts.hit();
                    prop_assert_eq!(error, &SpaceError::CellOccupied);
                    prop_assert!(obs.events.is_empty(), "an abort stages nothing");
                    prop_assert_eq!(obs.before.tokens(), obs.after.tokens());
                    prop_assert_eq!(&obs.counter, &counter, "an abort bumps nothing");
                } else {
                    counter = obs.counter.clone();
                }
            }
            Ok(())
        })
    });
    aborts.assert_at_least(32, "a store or a park on a full cell aborts");
}

/// V8 (`PeekRestoresRecord`): every produce on the data channel registers the authority of its
/// own datum, and a bag, a set or a cell stores a record only when no parked waiter binds it and
/// admits it.
#[test]
fn prop_engine_peek_restores_record() {
    check(pure_config(), script(), |script| {
        for_families(
            &[Family::Queue, Family::Bag, Family::Cell, Family::Set],
            &script,
            |trace| {
                for obs in &trace.observations {
                    if obs.result.is_err() {
                        continue;
                    }
                    let tokens: BTreeMap<_, _> = obs
                        .after
                        .records
                        .iter()
                        .map(|record| (record.raw.source.hash.clone(), record.token.clone()))
                        .collect();
                    for (produce, authority) in &obs.registered {
                        if let Some(token) = tokens.get(&produce.hash) {
                            let own = trace.sent.get(token).map(|sent| sent.authority.clone());
                            prop_assert_eq!(
                                authority,
                                &own,
                                "a record registers its own authority"
                            );
                        }
                    }
                    if trace.family == Family::Queue {
                        continue;
                    }
                    let peeked: BTreeSet<Vec<u8>> = obs
                        .fires
                        .iter()
                        .filter(|fire| consume_side(obs, fire) && receiver_of(trace, fire).peek)
                        .flat_map(|fire| fire.results.iter())
                        .filter(|result| !result.persistent)
                        .map(|result| result.token.clone())
                        .collect();
                    let fresh: Vec<&Rec> = obs
                        .after
                        .records
                        .iter()
                        .filter(|record| {
                            obs.before.record(&record.token).is_none()
                                || peeked.contains(&record.token)
                        })
                        .collect();
                    for record in fresh {
                        prop_assert!(
                            !obs.after
                                .waiters
                                .iter()
                                .any(|waiter| takes(waiter, &record.payload)),
                            "a record is stored only when no parked waiter admits it"
                        );
                    }
                }
                Ok(())
            },
        )
    });
}

// ── A deterministic case: the growth of the staged list ────────────────────────────────────

/// A set with seventy stored members of one class, one record of another class first in the
/// canonical order, and a guarded waiter that refused that record. A send of the class fires the
/// waiter and retires the seventy members, so the operation stages more than the head-only room
/// of the staged list. The operation commits, and only the record of the other class remains.
#[test]
fn engine_set_retirement_grows_the_staged_list() {
    const MEMBERS: usize = 70;
    let runtime = tokio_runtime();
    let build = || async {
        let host = host_with(Injected::Real).await;
        let budget = budget();
        let env = env_over(host.clone(), &budget);
        let agent = GenericSpace::<HashMapOuter, SetDiscipline>::default();
        let config = config_of(
            SPACE,
            HashMapOuter::NAME,
            SetDiscipline::NAME,
            SpaceQualifier::Default,
        );
        create(
            &agent,
            &env,
            creation(SPACE, config, SpaceFactoryUrn::HASHMAP_QUEUE),
        )
        .await;
        let channel = string("rrs-engine");
        let data = data_channel(SPACE, &channel);
        for index in 0..MEMBERS {
            run(
                &agent,
                &env,
                SpaceOp::Produce(send_op(&channel, index, 1, false)),
            )
            .await
            .expect("a member stores");
        }
        // A record of value 2 that comes first in the canonical order.
        let mut index = MEMBERS;
        loop {
            run(
                &agent,
                &env,
                SpaceOp::Produce(send_op(&channel, index, 2, false)),
            )
            .await
            .expect("the other record stores");
            let stored: Vec<_> = host.get_data(&data).await;
            let order = canonical_data_order(&stored);
            let first = record::unwrap_datum(&stored[order[0]].a)
                .expect("a record")
                .datum;
            if value_of(&first) == 2 {
                break;
            }
            run(
                &agent,
                &env,
                SpaceOp::Consume(receive_op(&channel, index, Wanted::Two, false, false)),
            )
            .await
            .expect("the other record leaves");
            index += 1;
            assert!(
                index < MEMBERS + 1_000,
                "a record of value 2 comes first in time"
            );
        }
        // The guarded waiter: its first spatial match is the record of value 2, which its guard
        // `x == 1` refuses, so it parks (decision 37).
        let guarded = ConsumeOp {
            guard: Some(bound_equals(1)),
            ..receive_op(&channel, index + 1, Wanted::Any, false, false)
        };
        run(&agent, &env, SpaceOp::Consume(guarded))
            .await
            .expect("the waiter parks");
        assert_eq!(
            host.get_waiting_continuations(vec![data.clone()])
                .await
                .len(),
            1
        );
        host.take_event_log().await;
        (host, budget, agent, channel, data, index + 2)
    };
    let (host, budget, agent, channel, data, next) = runtime.block_on(build());
    let outcome = runtime
        .block_on(run(
            &agent,
            &env_over(host.clone(), &budget),
            SpaceOp::Produce(send_op(&channel, next, 1, false)),
        ))
        .expect("the send fires and retires the class");
    assert_eq!(outcome.fires.len(), 1, "the send fires the guarded waiter");
    let log = runtime.block_on(host.take_event_log());
    let comms = log
        .iter()
        .filter(|event| matches!(event, Event::Comm(_)))
        .count();
    assert_eq!(
        comms,
        MEMBERS + 1,
        "one fire and one K_retire for each member"
    );
    let left = runtime.block_on(snapshot(&host, &data));
    assert_eq!(left.records.len(), 1, "only the record of value 2 remains");
    assert_eq!(left.records[0].value, 2);
}

// ── A deterministic case: the second send fires a guarded waiter (Rule E) ───────────────────

/// Builds a collection of `D` with a record of value 1 first in the canonical order and a record
/// of value 2 after it, and a parked waiter of any value with the guard `x == 2`. The first
/// spatial match of the waiter is the record of value 1, which its guard refuses, so it parks
/// (decision 37). A peek of the record of value 2 removes it, and its second send then fires
/// the guarded waiter, as the host's `produce_peeks` does (decision 45, C.11).
async fn reoffer_witness<D: Discipline>() -> (Vec<Fired>, Snapshot) {
    let host = host_with(Injected::Real).await;
    let budget = budget();
    let env = env_over(host.clone(), &budget);
    let agent = GenericSpace::<HashMapOuter, D>::default();
    let config = config_of(SPACE, HashMapOuter::NAME, D::NAME, SpaceQualifier::Default);
    create(
        &agent,
        &env,
        creation(SPACE, config, SpaceFactoryUrn::HASHMAP_QUEUE),
    )
    .await;
    let channel = string("rrs-engine");
    let data = data_channel(SPACE, &channel);
    run(
        &agent,
        &env,
        SpaceOp::Produce(send_op(&channel, 0, 2, false)),
    )
    .await
    .expect("the record of value 2 stores");
    let mut index = 1;
    loop {
        run(
            &agent,
            &env,
            SpaceOp::Produce(send_op(&channel, index, 1, false)),
        )
        .await
        .expect("a record of value 1 stores");
        let stored = host.get_data(&data).await;
        let order = canonical_data_order(&stored);
        let first = record::unwrap_datum(&stored[order[0]].a)
            .expect("a record")
            .datum;
        if value_of(&first) == 1 {
            break;
        }
        run(
            &agent,
            &env,
            SpaceOp::Consume(receive_op(&channel, index, Wanted::One, false, false)),
        )
        .await
        .expect("the record of value 1 leaves");
        index += 1;
        assert!(index < 1_000, "a record of value 1 comes first in time");
    }
    run(
        &agent,
        &env,
        SpaceOp::Consume(receive_op(
            &channel,
            index + 1,
            Wanted::AnyTwo,
            false,
            false,
        )),
    )
    .await
    .expect("the guarded waiter parks");
    assert_eq!(
        host.get_waiting_continuations(vec![data.clone()])
            .await
            .len(),
        1
    );
    let outcome = run(
        &agent,
        &env,
        SpaceOp::Consume(receive_op(&channel, index + 2, Wanted::Two, false, true)),
    )
    .await
    .expect("the peek fires");
    (decode_fires(&outcome), snapshot(&host, &data).await)
}

/// Rule E with the re-offer of A.7: on a bag and on a set, the peek fires with the record of
/// value 2, and the second send of that record fires the guarded waiter. So the record leaves
/// the collection with the waiter, and the record of value 1 stays.
#[test]
fn engine_peek_restore_reoffers_to_a_guarded_waiter() {
    let runtime = tokio_runtime();
    for (fires, after) in [
        runtime.block_on(reoffer_witness::<BagDiscipline>()),
        runtime.block_on(reoffer_witness::<SetDiscipline>()),
    ] {
        assert_eq!(
            fires.len(),
            2,
            "the peek fire and the fire of its second send"
        );
        assert!(fires[0].name != fires[1].name);
        assert!(fires.iter().all(|fire| fire.results[0].value == 2));
        assert!(after.waiters.is_empty(), "the guarded waiter fired");
        assert_eq!(after.records.len(), 1);
        assert_eq!(after.records[0].value, 1, "the record of value 1 stays");
    }
}

// ── Negative controls of the oracles ────────────────────────────────────────────────────────

/// The oracle of V1 detects a pending match: a queue blocks at its head, so a parked waiter for
/// 2 without a guard waits behind the head record 1 while the record 2 is queued (decision 11).
/// A bag never reaches that state on the same script.
#[test]
fn engine_oracle_detects_a_pending_match_behind_a_queue_head() {
    let script = [
        Step::Send {
            value: 1,
            persistent: false,
        },
        Step::Receive {
            wanted: Wanted::Two,
            persistent: false,
            peek: false,
        },
        Step::Send {
            value: 2,
            persistent: false,
        },
    ];
    let runtime = tokio_runtime();
    let queue = runtime.block_on(run_script(
        Setup {
            family: Family::Queue,
            injected: Injected::Real,
        },
        &script,
    ));
    let last = &queue.observations.last().expect("three operations").after;
    assert!(
        guard_free_pending(last),
        "the oracle sees the match behind the head"
    );
    let bag = runtime.block_on(run_script(
        Setup {
            family: Family::Bag,
            injected: Injected::Real,
        },
        &script,
    ));
    assert!(bag
        .observations
        .iter()
        .all(|obs| !guard_free_pending(&obs.after)));
}

/// The oracles of V3 and of F18 detect a broken run: a record that disappears without a fire,
/// and a fire of a waiter that is not the first admitting waiter in canonical order.
#[test]
fn engine_oracles_detect_a_lost_record_and_a_wrong_choice() {
    let script = [
        Step::Receive {
            wanted: Wanted::Any,
            persistent: false,
            peek: false,
        },
        Step::Receive {
            wanted: Wanted::One,
            persistent: false,
            peek: false,
        },
        Step::Send {
            value: 2,
            persistent: false,
        },
        Step::Send {
            value: 1,
            persistent: false,
        },
    ];
    let trace = tokio_runtime().block_on(run_script(
        Setup {
            family: Family::Bag,
            injected: Injected::Real,
        },
        &script,
    ));
    // The first send fires the waiter of any value, and the second send fires the waiter of 1.
    let first = &trace.observations[2];
    assert_eq!(first.fires.len(), 1);
    assert!(check_fates(&trace, first, &BTreeSet::new()).is_err());
    let mut spent = BTreeSet::new();
    spent.extend(spent_by(&trace, &first.fires[0]));
    assert!(check_fates(&trace, first, &spent).is_ok());
    // A stored record that disappears with no fire breaks conservation.
    let store = Observation {
        op: SpaceOp::Produce(send_op(&string("rrs-engine"), 9, 1, false)),
        before: Snapshot {
            records: trace.observations[3].before.records.clone(),
            waiters: Vec::new(),
        },
        after: Snapshot {
            records: Vec::new(),
            waiters: Vec::new(),
        },
        result: Ok(SpaceOutcome::default()),
        fires: Vec::new(),
        events: Vec::new(),
        registered: Vec::new(),
        in_flight: BTreeSet::new(),
        counter: Vec::new(),
    };
    assert!(check_fates(&trace, &store, &spent).is_err());
    // A fire that names another waiter breaks the choice of decision 37.
    let second = &trace.observations[3];
    let SpaceOp::Produce(produce) = &second.op else {
        panic!("the fourth operation is a send")
    };
    let skipped = Hits::new();
    assert!(check_offer_choices(second, &produce.data, &skipped).is_ok());
    let mut wrong = second.fires.clone();
    wrong[0].name = receiver(0);
    let renamed = Observation {
        op: second.op.clone(),
        before: second.before.clone(),
        after: second.after.clone(),
        result: Ok(SpaceOutcome::default()),
        fires: wrong,
        events: Vec::new(),
        registered: Vec::new(),
        in_flight: BTreeSet::new(),
        counter: Vec::new(),
    };
    assert!(check_offer_choices(&renamed, &produce.data, &skipped).is_err());
}
