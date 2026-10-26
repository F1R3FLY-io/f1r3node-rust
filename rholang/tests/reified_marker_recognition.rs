// P2 tests of the Reified RSpaces recognition hooks (plan decision 22), with the P4 codes.
//
// The property tests have the names that formal/reified-rspaces-invariant-map.json maps to
// the headlines E7-E12 of formal/rocq/reified_rspaces/theories/Encoding.v. Each fixture is a
// Par that the models builders build, and the reducer evaluates it with `inj`. Every
// recognized construct fails closed with a typed `reified:<Code>:` error, or it translates. No
// space reference of a fixture names a native space (P4 design C.2 and C.3). The empty Par
// gives `EmptySpaceRef`. Every other value names a user agent, and from P8 step 4 the construct
// translates: a USE block runs its body in the context of the agent, and a NEWSPACE tuple with
// an agent annotation makes a gensym call and parks its resume term (`hook_outcome`). Before
// the strict decode, the hooks recompute the flags that the encoder derives (I-20). Two tests
// check consensus checklist items 18 and 19 for P2.

#[path = "../../models/tests/support/reified_strategies.rs"]
mod reified_strategies;

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::{VarInstance, WildcardMsg};
use models::rhoapi::{
    BindPattern, Bundle, EEq, EMethod, ETuple, EVar, Expr, If, ListParWithRandom, Match, MatchCase,
    New, Par, Receive, ReceiveBind, TaggedContinuation, Var,
};
use models::rust::reified::builders::{push_process_expr, send_params, use_block};
use models::rust::reified::encoding::{
    decode_free_value, decode_process_tuple, decode_query_source, encode, encode_new_space,
    marker_of, split_trailing_params, Marker, NewSpace, ReifiedConstruct, ReifiedDecodeError,
};
use models::rust::reified::params::SendParam;
use models::rust::rholang::implicits::concatenate_pars;
use proptest::prelude::*;
use proptest::sample::select;
use proptest::test_runner::{RngSeed, TestRunner};
use prost::Message;
use reified_strategies::{
    arb_construct, arb_ground_construct, arb_ground_par, arb_named, arb_par, bound_var,
    contains_marker, gbool, gint, gstring, int_connective, list, map, new_term, position_par,
    private_name, send, sole_tuple, tuple,
};
use rholang::rust::interpreter::accounting::cost_accounting::CostAccounting;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::lexical::resolve_lexical_names_for_funding;
use rholang::rust::interpreter::accounting::RuntimeBudget;
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::env::Env;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::merging::mergeable_tags::{
    default_mergeable_tags, non_negative_mergeable_tag_name,
};
use rholang::rust::interpreter::metering::MeteredMachine;
use rholang::rust::interpreter::reduce::DebruijnInterpreter;
use rholang::rust::interpreter::registry::registry_bootstrap;
use rholang::rust::interpreter::rho_runtime::RhoISpace;
use rholang::rust::interpreter::substitute::{Substitute, SubstituteTrait};
use rholang::rust::interpreter::system_processes::byte_name;
use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rholang::rust::interpreter::util::{
    evaluation_random, evaluation_terms, is_evaluated_process_expr, owned_evaluation_terms,
    prepend_expr, prepend_new, GeneratedMessage,
};
use rspace_plus_plus::rspace::hot_store::HotStoreState;
use rspace_plus_plus::rspace::merger::merging_logic::MergeType;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::trace::event::{Event, IOEvent};

const PROPERTY_SEED: u64 = 0x0E0D_2026_1004_0007;
const LEXICAL_DISAGREEMENT: &str = "funding resolver term schedule disagrees with reducer schedule";
/// The contract of the second HEAD empty-state test (casper/tests/util/rholang/runtime_spec.rs).
const RUNTIME_SPEC_CONTRACT: &str = r#"new a in {
           @"2"!(10)|
           @2!("test")|
           @"3"!!(3)|
           @42!!("1")|
           for (@t <- a){Nil}|
           for (@num <- @"3"&@num2 <- @1){10}|
           for (@_ <= @"4"){"3"}|
           for (@_ <= @"5"& @num3 <= @5){Nil}|
           for (@3 <- @44){new g in {Nil}}|
           for (@_ <- @"55"& @num3 <- @55){Nil}
         }"#;

type ParSpace = RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

fn property_config() -> ProptestConfig {
    ProptestConfig {
        cases: 64,
        rng_seed: RngSeed::Fixed(PROPERTY_SEED),
        failure_persistence: None,
        ..ProptestConfig::default()
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("build the test runtime")
}

fn rand() -> Blake2b512Random { Blake2b512Random::create_from_bytes(b"rrs-p2-marker-recognition") }

fn substitute_instance() -> Substitute {
    Substitute {
        metering: MeteredMachine::new(RuntimeBudget::new(Cost::unsafe_max())),
    }
}

struct Harness {
    space: ParSpace,
    reducer: Arc<DebruijnInterpreter>,
    merge_chs: Arc<tokio::sync::RwLock<HashMap<Par, MergeType>>>,
}

async fn harness(mergeable_tags: HashMap<Par, MergeType>) -> Harness {
    let mut stores = InMemoryStoreManager::new();
    let store = stores
        .r_space_stores()
        .await
        .expect("create the in-memory RSpace stores");
    let space: ParSpace =
        RSpace::create(store, Arc::new(Box::new(Matcher))).expect("create the RSpace");
    let rspace: RhoISpace = Arc::new(Box::new(space.clone()));
    let merge_chs = Arc::new(tokio::sync::RwLock::new(HashMap::new()));
    let cost = CostAccounting::empty_cost();
    let reducer = DebruijnInterpreter::new(
        rspace,
        Arc::new(HashMap::new()),
        merge_chs.clone(),
        Arc::new(mergeable_tags),
        cost.clone(),
    );
    cost.set(Cost::create(
        i64::MAX,
        "rrs-p2 marker recognition".to_string(),
    ));
    Harness {
        space,
        reducer,
        merge_chs,
    }
}

/// The observable result of one evaluation.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Outcome {
    error: Option<String>,
    store: Vec<Vec<u8>>,
    produces: usize,
    consumes: usize,
    comms: usize,
    /// The number of channels of the widest join of the hot store, or 0 without a join.
    widest_join: usize,
}

impl Outcome {
    fn code(&self) -> Option<&str> {
        self.error.as_deref().and_then(|error| {
            error
                .strip_prefix("reified:")
                .and_then(|rest| rest.split(':').next())
        })
    }

    fn is_quiet(&self) -> bool {
        self.store.is_empty() && self.produces == 0 && self.consumes == 0 && self.comms == 0
    }
}

fn error_text(error: &InterpreterError) -> String {
    match error {
        InterpreterError::ReduceError(message) => message.clone(),
        InterpreterError::AggregateError { interpreter_errors } => interpreter_errors
            .iter()
            .map(error_text)
            .collect::<Vec<_>>()
            .join(" | "),
        other => format!("{other:?}"),
    }
}

fn length_prefixed(bytes: &mut Vec<u8>, part: &[u8]) {
    bytes.extend((part.len() as u64).to_le_bytes());
    bytes.extend(part);
}

/// The hot store as sorted byte strings: the data, the waiting continuations of every channel
/// group and the joins. The record sources hold hashes, so they are left out.
fn store_entries(
    state: &HotStoreState<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
) -> Vec<Vec<u8>> {
    let mut entries = Vec::new();
    for shard in state.data.iter() {
        for (channel, data) in shard.iter().filter(|(_, data)| !data.is_empty()) {
            let mut bytes = vec![b'D'];
            length_prefixed(&mut bytes, &channel.encode_to_vec());
            for datum in data {
                length_prefixed(&mut bytes, &datum.a.encode_to_vec());
                bytes.push(u8::from(datum.persist));
            }
            entries.push(bytes);
        }
    }
    for shard in state.continuations.iter() {
        for (channels, waiting) in shard.iter().filter(|(_, waiting)| !waiting.is_empty()) {
            let mut bytes = vec![b'K'];
            for channel in channels {
                length_prefixed(&mut bytes, &channel.encode_to_vec());
            }
            for continuation in waiting {
                for pattern in &continuation.patterns {
                    length_prefixed(&mut bytes, &pattern.encode_to_vec());
                }
                length_prefixed(&mut bytes, &continuation.continuation.encode_to_vec());
                bytes.push(u8::from(continuation.persist));
                for peek in &continuation.peeks {
                    bytes.extend(peek.to_le_bytes());
                }
            }
            entries.push(bytes);
        }
    }
    for shard in state.joins.iter() {
        for (channel, joins) in shard.iter().filter(|(_, joins)| !joins.is_empty()) {
            let mut bytes = vec![b'J'];
            length_prefixed(&mut bytes, &channel.encode_to_vec());
            let mut groups: Vec<Vec<u8>> = joins
                .iter()
                .map(|group| group.iter().flat_map(Message::encode_to_vec).collect())
                .collect();
            groups.sort();
            for group in groups {
                length_prefixed(&mut bytes, &group);
            }
            entries.push(bytes);
        }
    }
    entries.sort();
    entries
}

async fn evaluate_with(
    program: Par,
    mergeable_tags: HashMap<Par, MergeType>,
) -> (Outcome, HashMap<Par, MergeType>) {
    let harness = harness(mergeable_tags).await;
    let error = harness
        .reducer
        .inj(program, rand())
        .await
        .err()
        .map(|error| error_text(&error));
    let checkpoint = harness.space.create_soft_checkpoint().await;
    let widest_join = checkpoint
        .cache_snapshot
        .joins
        .iter()
        .flat_map(|shard| {
            shard
                .iter()
                .flat_map(|(_, joins)| joins.iter().map(Vec::len))
                .collect::<Vec<_>>()
        })
        .max()
        .unwrap_or(0);
    let mut outcome = Outcome {
        error,
        store: store_entries(&checkpoint.cache_snapshot),
        produces: 0,
        consumes: 0,
        comms: 0,
        widest_join,
    };
    for event in &checkpoint.log {
        match event {
            Event::IoEvent(IOEvent::Produce(_)) => outcome.produces += 1,
            Event::IoEvent(IOEvent::Consume(_)) => outcome.consumes += 1,
            Event::Comm(_) => outcome.comms += 1,
        }
    }
    let mergeable = harness.merge_chs.read().await.clone();
    (outcome, mergeable)
}

async fn evaluate(program: Par) -> Outcome { evaluate_with(program, HashMap::new()).await.0 }

fn expr_par(instance: ExprInstance, connective_used: bool) -> Par {
    reified_strategies::expr_par(instance, Vec::new(), connective_used)
}

fn wildcard() -> Par {
    expr_par(
        ExprInstance::EVarBody(EVar {
            v: Some(Var {
                var_instance: Some(VarInstance::Wildcard(WildcardMsg {})),
            }),
        }),
        true,
    )
}

fn free_var(index: i32) -> Par {
    expr_par(
        ExprInstance::EVarBody(EVar {
            v: Some(Var {
                var_instance: Some(VarInstance::FreeVar(index)),
            }),
        }),
        true,
    )
}

/// `for (patterns <- source) { body }` with `free_count` pattern variables.
fn receive(source: Par, patterns: Vec<Par>, free_count: i32, body: Par, persistent: bool) -> Par {
    join(
        vec![(source, patterns, free_count)],
        body,
        persistent,
        false,
    )
}

/// A receive with one bind for each `(source, patterns, free_count)`.
fn join(binds: Vec<(Par, Vec<Par>, i32)>, body: Par, persistent: bool, peek: bool) -> Par {
    let bind_count = binds.iter().map(|(_, _, free_count)| free_count).sum();
    let binds = binds
        .into_iter()
        .map(|(source, patterns, free_count)| ReceiveBind {
            patterns,
            source: Some(source),
            remainder: None,
            free_count,
            cost_signature: None,
        })
        .collect();
    Par {
        receives: vec![Receive {
            binds,
            body: Some(body),
            persistent,
            peek,
            bind_count,
            locally_free: Vec::new(),
            connective_used: false,
            condition: None,
        }],
        ..Par::default()
    }
}

fn persistent_send(chan: Par, data: Vec<Par>) -> Par {
    let mut par = send(chan, data);
    for sent in &mut par.sends {
        sent.persistent = true;
    }
    par
}

fn new_par(bind_count: i32, body: Par) -> Par {
    Par {
        news: vec![New {
            bind_count,
            p: Some(body),
            uri: Vec::new(),
            injections: Default::default(),
            locally_free: Vec::new(),
        }],
        ..Par::default()
    }
}

fn bundle_par(body: Par) -> Par {
    Par {
        bundles: vec![Bundle {
            body: Some(body),
            write_flag: true,
            read_flag: true,
        }],
        ..Par::default()
    }
}

fn if_par(condition: Par, if_true: Par, if_false: Par) -> Par {
    Par {
        conditionals: vec![If {
            condition: Some(condition),
            if_true: Some(if_true),
            if_false: Some(if_false),
            locally_free: Vec::new(),
            connective_used: false,
        }],
        ..Par::default()
    }
}

fn match_par(target: Par, pattern: Par, source: Par) -> Par {
    Par {
        matches: vec![Match {
            target: Some(target),
            cases: vec![MatchCase {
                pattern: Some(pattern),
                source: Some(source),
                free_count: 0,
                guard: None,
            }],
            locally_free: Vec::new(),
            connective_used: false,
        }],
        ..Par::default()
    }
}

fn equality(left: Par, right: Par) -> Par {
    expr_par(
        ExprInstance::EEqBody(EEq {
            p1: Some(left),
            p2: Some(right),
        }),
        false,
    )
}

fn method_nth(target: Par, index: i64) -> Par {
    expr_par(
        ExprInstance::EMethodBody(EMethod {
            method_name: "nth".to_owned(),
            target: Some(target),
            arguments: vec![gint(index)],
            locally_free: Vec::new(),
            connective_used: false,
        }),
        false,
    )
}

fn tuple_of(par: &Par) -> ETuple {
    sole_tuple(par)
        .expect("the Par holds exactly one tuple")
        .clone()
}

fn tuple_expr(tuple: ETuple) -> Expr {
    Expr {
        expr_instance: Some(ExprInstance::ETupleBody(tuple)),
    }
}

fn process(tuple: ETuple) -> Par { push_process_expr(Par::default(), tuple) }

/// The typed error of a USE or NEWSPACE hook for a reference without components.
const EMPTY_SPACE_REF: &str = "reified:EmptySpaceRef: the space reference is empty";
// Disabled by P8 step 4 (design v2 AG-2, W4): a USE or NEWSPACE construct of a user agent
// translates, so no hook raises this error (`hook_outcome`).
// /// The typed error of a USE or NEWSPACE hook for a space of a user agent (P8).
// const AGENT_SPACES_UNAVAILABLE: &str =
//     "reified:AgentSpacesUnavailable: spaces that user agents implement are not available";

/// True when `par` holds no process and no expression. The bit sets do not count.
fn is_empty_par(par: &Par) -> bool {
    Par {
        locally_free: Vec::new(),
        connective_used: false,
        ..par.clone()
    } == Par::default()
}

// Replaced by `hook_outcome` in P8 step 4 (design v2 AG-2, W4): the constructs of user agents
// translate instead of failing closed.
// /// The error of the P4 hook H2 for a USE or NEWSPACE construct whose space references name no
// /// native space (P4 design C.2 and C.3). The hook classifies the space of a USE, or the
// /// annotations of a NEWSPACE in binder order, and the first reference fails closed. A fixture
// /// holds no bundle and no Reified id. So the empty `Par` is `EmptySpaceRef`, and every other
// /// value is a space of a user agent. Evaluation keeps each component of a reference, so the
// /// classification of the evaluated reference is the classification of the fixture.
// fn space_ref_error(construct: &ReifiedConstruct) -> Option<&'static str> {
//     let reference = match construct {
//         ReifiedConstruct::Use(block) => &block.space,
//         ReifiedConstruct::NewSpace(new_space) => new_space.annotations.iter().flatten().next()?,
//         ReifiedConstruct::Params(_) | ReifiedConstruct::Query(_) | ReifiedConstruct::Free(_) => {
//             return None
//         }
//     };
//     Some(match is_empty_par(reference) {
//         true => EMPTY_SPACE_REF,
//         false => AGENT_SPACES_UNAVAILABLE,
//     })
// }

/// What the P4 hook H2 does with a USE or NEWSPACE construct whose space references name no
/// native space (P4 design C.2 and C.3, P8 design v2 AG-2 and W4).
#[derive(Clone, Debug, PartialEq, Eq)]
enum HookOutcome {
    /// The construct fails with this error before any effect: an empty reference, or a URI
    /// binder of a translated `new` that the URN map of the harness does not hold.
    Failed(String),
    /// Every reference names a user agent, and the construct translates (P8 step 4). A USE
    /// block runs its body in the context of the agent, so each send of the body becomes a call
    /// or reaches the default space, and the evaluation is quiet exactly when the body has no
    /// send. A NEWSPACE tuple with an agent annotation makes a gensym call and parks its resume
    /// term, so it is never quiet.
    Translated { quiet: bool },
}

/// The outcome of the hook for `construct`, or `None` for a construct of another position or a
/// NEWSPACE tuple without an annotation. The hook evaluates the space of a USE, or every
/// annotation of a NEWSPACE in binder order, before any effect. A fixture holds no bundle and no
/// Reified id. So the empty `Par` is `EmptySpaceRef`, and every other value names a user agent,
/// whose reference passes the write rule. Evaluation keeps each component of a reference, so the
/// classification of the evaluated reference is the classification of the fixture. The bodies
/// of the fixtures are ground: their only effects are sends. A translated `new` draws its names
/// as HEAD does first (TN-1), and the harness has no URN map, so its first URI binder fails with
/// the error of HEAD.
fn hook_outcome(construct: &ReifiedConstruct) -> Option<HookOutcome> {
    match construct {
        ReifiedConstruct::Use(block) => Some(match is_empty_par(&block.space) {
            true => HookOutcome::Failed(EMPTY_SPACE_REF.to_string()),
            false => HookOutcome::Translated {
                quiet: block.body.sends.is_empty(),
            },
        }),
        ReifiedConstruct::NewSpace(new_space) => {
            let mut annotations = new_space.annotations.iter().flatten().peekable();
            annotations.peek()?;
            Some(
                match (annotations.any(is_empty_par), new_space.new.uri.first()) {
                    (true, _) => HookOutcome::Failed(EMPTY_SPACE_REF.to_string()),
                    (false, Some(urn)) => HookOutcome::Failed(format!(
                        "BugFoundError({:?})",
                        format!(
                            "No value set for {urn}. This is a bug in the normalizer or on the \
                             path from it."
                        )
                    )),
                    (false, None) => HookOutcome::Translated { quiet: false },
                },
            )
        }
        ReifiedConstruct::Params(_) | ReifiedConstruct::Query(_) | ReifiedConstruct::Free(_) => {
            None
        }
    }
}

/// Checks `outcome` against the outcome `expected` of the hook.
fn check_hook_outcome(outcome: &Outcome, expected: &HookOutcome) -> Result<(), TestCaseError> {
    match expected {
        HookOutcome::Failed(error) => {
            prop_assert_eq!(outcome.error.as_deref(), Some(error.as_str()));
            prop_assert!(outcome.store.is_empty());
        }
        HookOutcome::Translated { quiet } => {
            prop_assert_eq!(&outcome.error, &None);
            prop_assert_eq!(outcome.is_quiet(), *quiet, "{:?}", outcome);
            prop_assert!(outcome.widest_join < 2);
        }
    }
    Ok(())
}

/// A tuple `Par` with a marker head and the canonical flags of its elements.
fn marker_tuple(marker: Marker, fields: Vec<Par>) -> Par {
    let mut ps = Vec::with_capacity(fields.len() + 1);
    ps.push(marker.par());
    ps.extend(fields);
    tuple(ps)
}

fn sample_use() -> Par {
    marker_tuple(Marker::Use, vec![
        private_name(vec![3; 32]),
        send(gint(1), vec![gint(2)]),
    ])
}

fn sample_new_space() -> Par {
    process(
        match encode_new_space(&NewSpace {
            annotations: vec![Some(private_name(vec![4; 32]))],
            new: new_term(1, Vec::new(), send(gint(1), vec![gint(2)])),
        })
        .expect("an annotated new encodes")
        .expr_instance
        {
            Some(ExprInstance::ETupleBody(tuple)) => tuple,
            other => panic!("encode_new_space returned {other:?}"),
        },
    )
}

fn sample_params() -> Par {
    send_params(vec![
        SendParam::Positional(gint(3)),
        SendParam::Named("priority".to_owned(), gint(4)),
    ])
    .expect("params build")
    .expect("params are not empty")
}

fn sample_query() -> Par {
    marker_tuple(Marker::Query, vec![
        gstring("rrs-p2-query"),
        list(vec![gint(5)]),
        map(vec![("k".to_owned(), gint(1))]),
    ])
}

// ── E7 ─────────────────────────────────────────────────────────────────────

/// E7 (recognize_complete): each well-formed construct is recognized as itself in its own
/// position, and each hook position ignores the constructs of the other positions.
#[test]
fn marker_tuple_recognized_only_in_specified_positions() {
    let runtime = runtime();
    let mut runner = TestRunner::new(property_config());
    runner
        .run(&arb_ground_construct(), |construct| {
            runtime.block_on(check_recognized_in_place(&construct))
        })
        .expect("each construct is recognized in its own position only");
}

async fn check_recognized_in_place(construct: &ReifiedConstruct) -> Result<(), TestCaseError> {
    let par = position_par(construct);
    let marker = construct.marker();
    let channel = gstring("rrs-p2-e7");
    match construct {
        ReifiedConstruct::Use(_) | ReifiedConstruct::NewSpace(_) => {
            prop_assert_eq!(
                decode_process_tuple(&tuple_of(&par)).map(ReifiedConstruct::from),
                Ok(construct.clone())
            );
            // Changed by P8 step 4 (design v2 AG-2, W4): the hook evaluates the reference fields
            // of this construct, and an empty reference fails closed while a user agent
            // translates (`hook_outcome`).
            // let expected = space_ref_error(construct);
            // prop_assert!(expected.is_some());
            let expected = hook_outcome(construct);
            prop_assert!(expected.is_some());
            let expected = expected.expect("a USE or NEWSPACE construct has an outcome");
            for program in [
                par.clone(),
                new_par(1, par.clone()),
                bundle_par(par.clone()),
            ] {
                let outcome = evaluate(program).await;
                // prop_assert_eq!(outcome.error.as_deref(), expected);
                // prop_assert!(outcome.store.is_empty());
                check_hook_outcome(&outcome, &expected)?;
            }
        }
        ReifiedConstruct::Params(params) => {
            prop_assert_eq!(
                split_trailing_params(&[gint(1), par.clone()]),
                Ok(Some(params.clone()))
            );
            for program in [
                send(channel.clone(), vec![gint(1), par.clone()]),
                persistent_send(channel.clone(), vec![par.clone()]),
            ] {
                let outcome = evaluate(program).await;
                prop_assert_eq!(outcome.code(), Some("ParamsUnsupported"));
                prop_assert!(outcome.is_quiet());
            }
        }
        ReifiedConstruct::Query(query) => {
            prop_assert_eq!(decode_query_source(&par), Ok(Some(query.clone())));
            let outcome = evaluate(receive(
                par.clone(),
                vec![wildcard()],
                0,
                Par::default(),
                false,
            ))
            .await;
            prop_assert_eq!(outcome.code(), Some("QuerySpecUnsupported"));
            prop_assert!(outcome.is_quiet());
        }
        ReifiedConstruct::Free(theory) => {
            prop_assert_eq!(decode_free_value(&par), Ok(Some(theory.clone())));
        }
    }
    // The other positions are inert: HEAD behavior applies.
    if !matches!(marker, Marker::Use | Marker::NewSpace) {
        let outcome = evaluate(par.clone()).await;
        prop_assert_eq!(&outcome.error, &None);
        prop_assert!(outcome.is_quiet());
    }
    if marker != Marker::Params {
        let outcome = evaluate(send(channel.clone(), vec![gint(1), par.clone()])).await;
        prop_assert_eq!(&outcome.error, &None);
        prop_assert_eq!(outcome.produces, 1);
        prop_assert!(!outcome.store.is_empty());
    }
    if marker != Marker::Query {
        let outcome = evaluate(receive(
            par.clone(),
            vec![wildcard()],
            0,
            Par::default(),
            false,
        ))
        .await;
        prop_assert_eq!(&outcome.error, &None);
        prop_assert_eq!(outcome.consumes, 1);
        prop_assert!(!outcome.store.is_empty());
    }
    Ok(())
}

// ── E8 ─────────────────────────────────────────────────────────────────────

/// A field of a forged tuple.
fn arb_forged_field() -> BoxedStrategy<Par> {
    prop_oneof![
        arb_ground_par(),
        proptest::collection::vec(arb_ground_par(), 0..3).prop_map(list),
        arb_named(arb_ground_par()).prop_map(|named| map(named.into_iter().collect())),
        select(vec!["Nat", "new", "x"]).prop_map(gstring),
        Just(Par {
            news: vec![new_term(1, Vec::new(), Par::default())],
            ..Par::default()
        }),
    ]
    .boxed()
}

/// A tuple with a marker head: the canonical encoding of a construct, random fields of any
/// arity, or a canonical encoding with a flipped flag.
fn arb_forged() -> BoxedStrategy<ETuple> {
    let canonical =
        arb_ground_construct().prop_map(|construct| tuple_of(&position_par(&construct)));
    let random = (
        select(Marker::ALL.to_vec()),
        proptest::collection::vec(arb_forged_field(), 1..5),
    )
        .prop_map(|(marker, fields)| tuple_of(&marker_tuple(marker, fields)));
    let flagged = canonical.clone().prop_map(|mut tuple| {
        tuple.connective_used = !tuple.connective_used;
        tuple
    });
    prop_oneof![3 => canonical, 3 => random, 1 => flagged].boxed()
}

/// E8 (marker_forging_grants_no_capability): a term that a hook recognizes is the canonical
/// encoding of a well-formed construct that the position allows. The hook refreshes the
/// derived flags of the term first (I-20). So a forged term behaves as the syntax that it
/// equals. A malformed marker term fails closed.
#[test]
fn marker_forging_grants_no_capability() {
    let runtime = runtime();
    let mut runner = TestRunner::new(property_config());
    runner
        .run(&arb_forged(), |forged| {
            runtime.block_on(check_forged(forged))
        })
        .expect("forged marker tuples grant no capability");
}

async fn check_forged(forged: ETuple) -> Result<(), TestCaseError> {
    let marker = marker_of(&forged.ps[0]).expect("a forged tuple has a marker head");
    let forged_par = process(forged.clone());
    // I-20: before the strict decode, a hook recomputes the flags that the encoder derives.
    // The fields of a forged tuple carry their derived flags, so only the flag of the tuple
    // can be stale. `tuple` recomputes that flag from the fields. The decoders read a tuple of
    // another marker as no construct, whatever its flags are.
    let refreshed = tuple_of(&tuple(forged.ps.clone()));
    prop_assert_eq!(&refreshed.ps, &forged.ps);
    let refreshed_par = process(refreshed.clone());
    let channel = gstring("rrs-p2-e8");

    // Process position (H1 and H2).
    let outcome = evaluate(forged_par.clone()).await;
    match (marker, decode_process_tuple(&refreshed)) {
        (Marker::Use | Marker::NewSpace, Ok(construct)) => {
            let construct = ReifiedConstruct::from(construct);
            prop_assert_eq!(construct.marker(), marker);
            let canonical =
                encode(&construct).map_err(|error| TestCaseError::fail(error.to_string()))?;
            prop_assert_eq!(&canonical, &tuple_expr(refreshed.clone()));
            // Changed by P8 step 4 (design v2 AG-2, W4): see `hook_outcome`.
            // prop_assert_eq!(outcome.error.as_deref(), space_ref_error(&construct));
            match hook_outcome(&construct) {
                Some(expected) => check_hook_outcome(&outcome, &expected)?,
                None => prop_assert_eq!(&outcome.error, &None),
            }
            prop_assert_eq!(&outcome, &evaluate(position_par(&construct)).await);
        }
        (Marker::Use | Marker::NewSpace, Err(_)) => {
            prop_assert_eq!(outcome.code(), Some("Malformed"));
            prop_assert!(outcome.is_quiet());
        }
        (Marker::Params | Marker::Query | Marker::Free, _) => {
            prop_assert_eq!(&outcome.error, &None);
            prop_assert!(outcome.is_quiet());
        }
    }

    // The last data element of a send (H3).
    let outcome = evaluate(send(channel.clone(), vec![forged_par.clone()])).await;
    match split_trailing_params(std::slice::from_ref(&refreshed_par)) {
        Ok(Some(params)) => {
            let construct = ReifiedConstruct::Params(params);
            prop_assert_eq!(&position_par(&construct), &refreshed_par);
            prop_assert_eq!(outcome.code(), Some("ParamsUnsupported"));
            let replayed = evaluate(send(channel.clone(), vec![position_par(&construct)])).await;
            prop_assert_eq!(&outcome, &replayed);
        }
        Err(_) => prop_assert_eq!(outcome.code(), Some("Malformed")),
        Ok(None) => {
            prop_assert_eq!(&outcome.error, &None);
            prop_assert_eq!(outcome.produces, 1);
        }
    }

    // The source of a receive bind (H4).
    let outcome = evaluate(receive(
        forged_par.clone(),
        vec![wildcard()],
        0,
        Par::default(),
        false,
    ))
    .await;
    match decode_query_source(&refreshed_par) {
        Ok(Some(query)) => {
            let construct = ReifiedConstruct::Query(query);
            prop_assert_eq!(&position_par(&construct), &refreshed_par);
            prop_assert_eq!(outcome.code(), Some("QuerySpecUnsupported"));
            let replayed = evaluate(receive(
                position_par(&construct),
                vec![wildcard()],
                0,
                Par::default(),
                false,
            ))
            .await;
            prop_assert_eq!(&outcome, &replayed);
        }
        Err(_) => prop_assert_eq!(outcome.code(), Some("Malformed")),
        Ok(None) => {
            prop_assert_eq!(&outcome.error, &None);
            prop_assert_eq!(outcome.consumes, 1);
        }
    }
    Ok(())
}

// ── E9 ─────────────────────────────────────────────────────────────────────

/// The unused id `[0xE0, m + 5]` that stands in for the marker `m`.
fn decoy_index(marker: Marker) -> u8 { marker as u8 + 5 }

/// Replaces every `GPrivate` id `[0xE0, m + 5]` by `[0xE0, m]` in prost bytes.
fn rename_decoys(bytes: &[u8]) -> Vec<u8> {
    let mut renamed = bytes.to_vec();
    for marker in Marker::ALL {
        let decoy = [0x0a, 0x02, 0xE0, decoy_index(marker)];
        let mut index = 0;
        while index + decoy.len() <= renamed.len() {
            if renamed[index..index + decoy.len()] == decoy {
                renamed[index + 3] = marker as u8;
                index += decoy.len();
            } else {
                index += 1;
            }
        }
    }
    renamed
}

/// A program with the tuple `marked` in the inert positions that `mask` selects.
fn inert_program(marked: &Par, marker: Marker, mask: u16) -> Par {
    let chosen = |bit: u16| mask & (1 << bit) != 0;
    let out = |label: &str| gstring(&format!("rrs-p2-e9-out-{label}"));
    let mut pieces = Vec::with_capacity(11);
    if chosen(0) {
        // A send channel.
        pieces.push(send(marked.clone(), vec![gint(1)]));
    }
    if chosen(1) {
        // A data element before the last one.
        pieces.push(send(gstring("rrs-p2-e9-data"), vec![
            marked.clone(),
            gint(1),
        ]));
    }
    if chosen(2) && marker != Marker::Params {
        // The last data element of a send, for a marker that is not PARAMS.
        pieces.push(send(gstring("rrs-p2-e9-last"), vec![
            gint(1),
            marked.clone(),
        ]));
    }
    if chosen(3) {
        // Collection elements.
        pieces.push(send(gstring("rrs-p2-e9-collections"), vec![
            list(vec![marked.clone()]),
            tuple(vec![marked.clone(), gint(2)]),
            map(vec![("k".to_owned(), marked.clone())]),
        ]));
    }
    if chosen(4) {
        // A pattern, and the data that it matches.
        pieces.push(receive(
            gstring("rrs-p2-e9-pattern"),
            vec![marked.clone(), wildcard()],
            0,
            send(out("pattern"), vec![gint(4)]),
            false,
        ));
        pieces.push(send(gstring("rrs-p2-e9-pattern"), vec![
            marked.clone(),
            gint(0),
        ]));
    }
    if chosen(5) {
        // An if condition.
        pieces.push(if_par(
            equality(marked.clone(), marked.clone()),
            send(out("if"), vec![gint(5)]),
            Par::default(),
        ));
    }
    if chosen(6) {
        // A match target and a match pattern.
        pieces.push(match_par(
            marked.clone(),
            marked.clone(),
            send(out("match"), vec![gint(6)]),
        ));
    }
    if chosen(7) {
        // A method argument.
        pieces.push(send(gstring("rrs-p2-e9-method"), vec![
            method_nth(list(vec![marked.clone()]), 0),
            gint(7),
        ]));
    }
    if chosen(8) && !matches!(marker, Marker::Use | Marker::NewSpace) {
        // Process position, for a marker that the reducer does not evaluate there.
        pieces.push(marked.clone());
    }
    if chosen(9) && marker != Marker::Query {
        // A bind source, for a marker that is not QUERY.
        pieces.push(receive(
            marked.clone(),
            vec![wildcard()],
            0,
            Par::default(),
            false,
        ));
    }
    if chosen(10) {
        // The body of a new and of a bundle.
        pieces.push(new_par(
            1,
            send(gstring("rrs-p2-e9-new"), vec![marked.clone(), gint(9)]),
        ));
        pieces.push(bundle_par(send(gstring("rrs-p2-e9-bundle"), vec![
            marked.clone(),
            gint(10),
        ])));
    }
    pieces.into_iter().fold(Par::default(), |program, piece| {
        concatenate_pars(piece, program)
    })
}

/// E9 (marker_inert_elsewhere): a marker tuple outside its recognized position is ordinary
/// data. A program behaves the same, up to renaming, when an unused 2-byte decoy id replaces
/// the marker id.
#[test]
fn marker_inert_elsewhere() {
    let runtime = runtime();
    let mut runner = TestRunner::new(property_config());
    runner
        .run(
            &(arb_ground_construct(), 1u16..(1 << 11)),
            |(construct, mask)| runtime.block_on(check_inert(&construct, mask)),
        )
        .expect("marker tuples are inert outside their positions");
}

async fn check_inert(construct: &ReifiedConstruct, mask: u16) -> Result<(), TestCaseError> {
    let marker = construct.marker();
    let marked = position_par(construct);
    let mut decoy_tuple = tuple_of(&marked);
    decoy_tuple.ps[0] = private_name(vec![0xE0, decoy_index(marker)]);
    prop_assert_eq!(marker_of(&decoy_tuple.ps[0]), None);
    let decoy = process(decoy_tuple);

    let real = evaluate(inert_program(&marked, marker, mask)).await;
    let stand_in = evaluate(inert_program(&decoy, marker, mask)).await;
    prop_assert_eq!(&real.error, &None);
    prop_assert_eq!(&stand_in.error, &None);
    let mut renamed: Vec<Vec<u8>> = stand_in
        .store
        .iter()
        .map(|entry| rename_decoys(entry))
        .collect();
    renamed.sort();
    prop_assert_eq!(&real.store, &renamed);
    prop_assert_eq!(
        (real.produces, real.consumes, real.comms),
        (stand_in.produces, stand_in.consumes, stand_in.comms)
    );
    Ok(())
}

// ── E10 ────────────────────────────────────────────────────────────────────

/// The filter of HEAD (`util/mod.rs` at 4d67b5ccc, unchanged since 6f7a80da3): a variable or a
/// method call.
fn head_keeps(expr: &Expr) -> bool {
    matches!(
        expr.expr_instance,
        Some(ExprInstance::EVarBody(_)) | Some(ExprInstance::EMethodBody(_))
    )
}

/// `evaluation_terms` of HEAD, with the HEAD filter.
fn head_evaluation_terms(par: &Par) -> Vec<GeneratedMessage> {
    let mut terms = Vec::new();
    terms.extend(par.sends.iter().cloned().map(GeneratedMessage::Send));
    terms.extend(par.receives.iter().cloned().map(GeneratedMessage::Receive));
    terms.extend(par.news.iter().cloned().map(GeneratedMessage::New));
    terms.extend(par.matches.iter().cloned().map(GeneratedMessage::Match));
    terms.extend(par.conditionals.iter().cloned().map(GeneratedMessage::If));
    terms.extend(par.bundles.iter().cloned().map(GeneratedMessage::Bundle));
    terms.extend(
        par.exprs
            .iter()
            .filter(|expr| head_keeps(expr))
            .cloned()
            .map(GeneratedMessage::Expr),
    );
    terms.extend(
        par.cost_signed_terms
            .iter()
            .cloned()
            .map(GeneratedMessage::CostSignedTerm),
    );
    terms.extend(
        par.cost_stacks
            .iter()
            .cloned()
            .map(GeneratedMessage::CostStack),
    );
    terms
}

/// Each term as its kind and its prost bytes.
fn term_keys(terms: &[GeneratedMessage]) -> Vec<(u8, Vec<u8>)> {
    terms
        .iter()
        .map(|term| match term {
            GeneratedMessage::Send(message) => (0, message.encode_to_vec()),
            GeneratedMessage::Receive(message) => (1, message.encode_to_vec()),
            GeneratedMessage::New(message) => (2, message.encode_to_vec()),
            GeneratedMessage::Match(message) => (3, message.encode_to_vec()),
            GeneratedMessage::Bundle(message) => (4, message.encode_to_vec()),
            GeneratedMessage::Expr(message) => (5, message.encode_to_vec()),
            GeneratedMessage::If(message) => (6, message.encode_to_vec()),
            GeneratedMessage::CostSignedTerm(message) => (7, message.encode_to_vec()),
            GeneratedMessage::CostStack(message) => (8, message.encode_to_vec()),
        })
        .collect()
}

fn random_splits(count: usize) -> Vec<Vec<u8>> {
    let random = rand();
    (0..count)
        .map(|index| {
            evaluation_random(&random, index, count)
                .expect("an index below the term count splits")
                .to_bytes()
        })
        .collect()
}

/// A process `Par` with every kind of term and no marker.
fn arb_process_par() -> BoxedStrategy<Par> {
    let expr = prop_oneof![
        (0i32..3).prop_map(bound_var),
        (arb_ground_par(), 0i64..2).prop_map(|(item, index)| method_nth(list(vec![item]), index)),
        arb_ground_par(),
    ];
    (
        proptest::collection::vec(expr, 0..5),
        proptest::collection::vec((arb_ground_par(), arb_ground_par()), 0..3),
        proptest::collection::vec(arb_ground_par(), 0..3),
        arb_ground_par(),
    )
        .prop_map(|(exprs, sends, bodies, target)| {
            let mut pieces = exprs;
            pieces.extend(
                sends
                    .into_iter()
                    .map(|(chan, value)| send(chan, vec![value])),
            );
            for (index, body) in bodies.into_iter().enumerate() {
                pieces.push(match index {
                    0 => receive(gstring("rrs-p2-e10"), vec![wildcard()], 0, body, false),
                    1 => new_par(1, bundle_par(body)),
                    _ => if_par(
                        gbool(true),
                        match_par(target.clone(), wildcard(), body),
                        Par::default(),
                    ),
                });
            }
            pieces
                .into_iter()
                .fold(Par::default(), |par, piece| concatenate_pars(piece, par))
        })
        .boxed()
}

/// E10 (schedule_unchanged_marker_free): for a Par without a marker, both filter twins keep
/// the terms of HEAD, so the term list, the term count and the random splits stay as at
/// HEAD. A Par with k USE or NEWSPACE tuples gets exactly k more terms, and the funding
/// resolver, which keeps the HEAD filter, then fails closed.
#[test]
fn term_schedule_unchanged_for_marker_free_pars() {
    let markers = proptest::collection::vec(
        prop_oneof![
            reified_strategies::arb_use_block(arb_ground_par()).prop_map(ReifiedConstruct::Use),
            reified_strategies::arb_new_space(arb_ground_par())
                .prop_map(ReifiedConstruct::NewSpace),
        ],
        0..3,
    );
    let mut runner = TestRunner::new(property_config());
    runner
        .run(&(arb_process_par(), markers), |(par, markers)| {
            prop_assert!(!contains_marker(&par));
            for expr in &par.exprs {
                prop_assert_eq!(is_evaluated_process_expr(expr), head_keeps(expr));
            }
            let head = term_keys(&head_evaluation_terms(&par));
            let borrowed = term_keys(&evaluation_terms(&par));
            let owned = term_keys(&owned_evaluation_terms(par.clone()));
            prop_assert_eq!(&borrowed, &head);
            prop_assert_eq!(&owned, &head);
            prop_assert_eq!(random_splits(borrowed.len()), random_splits(head.len()));
            prop_assert!(resolve_lexical_names_for_funding(&par, rand(), &HashMap::new()).is_ok());

            let marked = markers.iter().fold(par.clone(), |marked, construct| {
                push_process_expr(marked, tuple_of(&position_par(construct)))
            });
            let marked_terms = evaluation_terms(&marked).len();
            prop_assert_eq!(
                marked_terms,
                head_evaluation_terms(&marked).len() + markers.len()
            );
            prop_assert_eq!(
                term_keys(&owned_evaluation_terms(marked.clone())).len(),
                marked_terms
            );
            let resolved = resolve_lexical_names_for_funding(&marked, rand(), &HashMap::new());
            match markers.is_empty() {
                true => prop_assert!(resolved.is_ok()),
                false => prop_assert_eq!(
                    resolved,
                    Err(InterpreterError::BugFoundError(
                        LEXICAL_DISAGREEMENT.to_owned()
                    ))
                ),
            }
            Ok(())
        })
        .expect("marker-free Pars keep the HEAD term schedule");
}

// ── E11 ────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug)]
enum Substitution {
    /// `substitute_and_charge`: eval_send, unbundle_receive and eval_match.
    Sorted,
    /// `substitute_no_sort_and_charge`: the stored body of a receive.
    Unsorted,
}

fn substitute(par: &Par, env: &Env<Par>, variant: Substitution) -> Par {
    let substitute = substitute_instance();
    match variant {
        Substitution::Sorted => substitute.substitute(par.clone(), 0, env),
        Substitution::Unsorted => substitute.substitute_no_sort(par.clone(), 0, env),
    }
    .expect("substitution of a generated Par succeeds")
}

/// The construct with each element substituted, as `subst_construct` of Encoding.v defines it.
fn substitute_construct(
    construct: &ReifiedConstruct,
    env: &Env<Par>,
    variant: Substitution,
) -> ReifiedConstruct {
    let each = |pars: &[Par]| -> Vec<Par> {
        pars.iter()
            .map(|par| substitute(par, env, variant))
            .collect()
    };
    match construct {
        ReifiedConstruct::Use(block) => {
            ReifiedConstruct::Use(models::rust::reified::encoding::UseBlock {
                space: substitute(&block.space, env, variant),
                body: substitute(&block.body, env, variant),
            })
        }
        ReifiedConstruct::NewSpace(new_space) => {
            let new_par = Par {
                news: vec![new_space.new.clone()],
                ..Par::default()
            };
            let substituted = substitute(&new_par, env, variant);
            ReifiedConstruct::NewSpace(NewSpace {
                annotations: new_space
                    .annotations
                    .iter()
                    .map(|annotation| {
                        annotation
                            .as_ref()
                            .map(|space| substitute(space, env, variant))
                    })
                    .collect(),
                new: substituted.news[0].clone(),
            })
        }
        ReifiedConstruct::Params(params) => {
            ReifiedConstruct::Params(models::rust::reified::encoding::SendParams {
                positional: each(&params.positional),
                named: params
                    .named
                    .iter()
                    .map(|(key, value)| (key.clone(), substitute(value, env, variant)))
                    .collect(),
            })
        }
        ReifiedConstruct::Query(query) => {
            ReifiedConstruct::Query(models::rust::reified::encoding::QuerySpec {
                channel: substitute(&query.channel, env, variant),
                payload: each(&query.payload),
                named: query
                    .named
                    .iter()
                    .map(|(key, value)| (key.clone(), substitute(value, env, variant)))
                    .collect(),
            })
        }
        ReifiedConstruct::Free(theory) => {
            ReifiedConstruct::Free(models::rust::reified::encoding::FreeTheory {
                name: theory.name.clone(),
                args: each(&theory.args),
            })
        }
    }
}

fn decode_in_position(par: &Par, marker: Marker) -> Result<ReifiedConstruct, String> {
    let decoded = match marker {
        Marker::Use | Marker::NewSpace => decode_process_tuple(&tuple_of(par))
            .map(|construct| Some(ReifiedConstruct::from(construct))),
        Marker::Params => split_trailing_params(std::slice::from_ref(par))
            .map(|params| params.map(ReifiedConstruct::Params)),
        Marker::Query => decode_query_source(par).map(|query| query.map(ReifiedConstruct::Query)),
        Marker::Free => decode_free_value(par).map(|theory| theory.map(ReifiedConstruct::Free)),
    };
    match decoded {
        Ok(Some(construct)) => Ok(construct),
        other => Err(format!("{other:?}")),
    }
}

/// E11 (decode_commutes_with_subst): substitution maps the encoding of a construct to the
/// encoding of the substituted construct. Substitution keeps the `connective_used` flag of a
/// tuple, so the property holds for values without connectives. A connective value leaves the
/// flag stale, and the decoder then fails closed. Recognition in process position depends on
/// the substitution time (finding H-3).
#[test]
fn reified_decode_commutes_with_substitution() {
    let mut runner = TestRunner::new(property_config());
    runner
        .run(
            &(
                arb_construct(),
                proptest::collection::vec(arb_ground_par(), 3),
            ),
            |(construct, values)| {
                let env = values
                    .into_iter()
                    .fold(Env::new(), |mut env, value| env.put(value));
                let marker = construct.marker();
                let variants: &[Substitution] = match marker {
                    Marker::Use | Marker::NewSpace => {
                        &[Substitution::Sorted, Substitution::Unsorted]
                    }
                    Marker::Params | Marker::Query | Marker::Free => &[Substitution::Sorted],
                };
                for variant in variants {
                    let substituted = substitute(&position_par(&construct), &env, *variant);
                    prop_assert_eq!(
                        decode_in_position(&substituted, marker),
                        Ok(substitute_construct(&construct, &env, *variant)),
                        "{:?}",
                        variant
                    );
                }
                Ok(())
            },
        )
        .expect("decoding commutes with substitution");

    // A connective value leaves the tuple flag stale: the decoder fails closed.
    let with_variable = marker_tuple(Marker::Use, vec![gint(1), bound_var(0)]);
    let env = Env::new().put(int_connective());
    let stale = substitute(&with_variable, &env, Substitution::Sorted);
    assert!(!tuple_of(&stale).connective_used);
    assert!(tuple_of(&stale).ps[2].connective_used);
    assert_eq!(
        decode_process_tuple(&tuple_of(&stale)),
        Err(ReifiedDecodeError::NonCanonical {
            marker: Marker::Use
        })
    );

    // H-3: a tuple with a variable head is inert when the reducer evaluates it directly, and
    // recognized once the storage substitution of a receive body makes the head literal.
    let runtime = runtime();
    let variable_head = tuple(vec![
        bound_var(0),
        private_name(vec![6; 32]),
        send(gint(1), vec![gint(2)]),
    ]);
    let direct = concatenate_pars(
        receive(
            gstring("rrs-p2-h3-a"),
            vec![free_var(0)],
            1,
            variable_head.clone(),
            false,
        ),
        send(gstring("rrs-p2-h3-a"), vec![Marker::Use.par()]),
    );
    let outcome = runtime.block_on(evaluate(direct));
    assert_eq!(outcome.error, None);
    assert_eq!(outcome.comms, 1);
    let stored = concatenate_pars(
        concatenate_pars(
            receive(
                gstring("rrs-p2-h3-b"),
                vec![free_var(0)],
                1,
                receive(
                    gstring("rrs-p2-h3-c"),
                    vec![wildcard()],
                    0,
                    variable_head,
                    false,
                ),
                false,
            ),
            send(gstring("rrs-p2-h3-b"), vec![Marker::Use.par()]),
        ),
        send(gstring("rrs-p2-h3-c"), vec![gint(0)]),
    );
    let outcome = runtime.block_on(evaluate(stored));
    // Changed by P8 step 4 (design v2 AG-2, TS-1): the space of the recognized USE is an
    // ordinary name, so the block runs its body in the context of that agent, and the send on
    // the forged channel `1` becomes the call on the agent.
    // The space of the recognized USE is an ordinary name, so the hook fails closed at the
    // agent gate (P8).
    // assert_eq!(outcome.code(), Some("AgentSpacesUnavailable"));
    assert_eq!(outcome.error, None);
    assert_eq!(
        (outcome.produces, outcome.consumes, outcome.comms),
        (3, 2, 2)
    );
    assert!(data_channels(&outcome).contains(&private_name(vec![6; 32]).encode_to_vec()));
}

/// The channels of the data entries of the store of `outcome`, in their prost bytes.
fn data_channels(outcome: &Outcome) -> Vec<Vec<u8>> {
    outcome
        .store
        .iter()
        .filter(|entry| entry.first() == Some(&b'D'))
        .map(|entry| {
            let mut length = [0u8; 8];
            length.copy_from_slice(&entry[1..9]);
            let length = usize::try_from(u64::from_le_bytes(length)).expect("a small entry");
            entry[9..9 + length].to_vec()
        })
        .collect()
}

// ── E12 ────────────────────────────────────────────────────────────────────

/// The superseded "s_i or Nil" NEWSPACE form of the plan.
fn plan_new_space(annotations: &[Option<Par>], new: &New) -> Par {
    marker_tuple(Marker::NewSpace, vec![
        list(
            annotations
                .iter()
                .map(|annotation| annotation.clone().unwrap_or_default())
                .collect(),
        ),
        Par {
            news: vec![new.clone()],
            connective_used: new.p.as_ref().is_some_and(|body| body.connective_used),
            ..Par::default()
        },
    ])
}

fn corrected_new_space(annotations: Vec<Option<Par>>, new: &New) -> Par {
    position_par(&ReifiedConstruct::NewSpace(NewSpace {
        annotations,
        new: new.clone(),
    }))
}

/// E12 (plan_nil_annotation_not_injective): the superseded "s_i or Nil" encoding maps the
/// annotation Nil and the absence of an annotation to one term, and the substitution of Nil
/// for an annotation variable erases the annotation. The `[]` or `[s]` encoding keeps both
/// distinctions.
#[test]
fn reified_plan_nil_encoding_counterexample() {
    let space = private_name(vec![8; 32]);
    let new = new_term(2, Vec::new(), Par::default());
    let nil_annotated = [Some(Par::default()), Some(space.clone())];
    let unannotated = [None, Some(space.clone())];

    assert_eq!(
        plan_new_space(&nil_annotated, &new),
        plan_new_space(&unannotated, &new)
    );
    assert_ne!(nil_annotated, unannotated);
    assert_ne!(
        corrected_new_space(nil_annotated.to_vec(), &new),
        corrected_new_space(unannotated.to_vec(), &new)
    );

    let env = Env::new().put(Par::default());
    let variable = [Some(bound_var(0)), Some(space.clone())];
    for variant in [Substitution::Sorted, Substitution::Unsorted] {
        // The plan: a receive that gets Nil for s erases the annotation of `x : s`.
        assert_eq!(
            substitute(&plan_new_space(&variable, &new), &env, variant),
            plan_new_space(&unannotated, &new)
        );
        // The correction: the annotation becomes [Nil], which the decoder reads as Nil.
        let substituted = substitute(&corrected_new_space(variable.to_vec(), &new), &env, variant);
        assert_eq!(
            substituted,
            corrected_new_space(nil_annotated.to_vec(), &new)
        );
        match decode_process_tuple(&tuple_of(&substituted)) {
            Ok(models::rust::reified::encoding::ProcessConstruct::NewSpace(decoded)) => {
                assert_eq!(decoded.annotations, nil_annotated.to_vec());
            }
            other => panic!("the substituted NEWSPACE tuple decodes, got {other:?}"),
        }
    }
    // The decoder rejects the plan form outright.
    assert_eq!(
        decode_process_tuple(&tuple_of(&plan_new_space(&nil_annotated, &new))),
        Err(ReifiedDecodeError::AnnotationShape { binder: 0 })
    );
}

// ── Hook errors ────────────────────────────────────────────────────────────

/// Each hook fails closed with `InterpreterError::ReduceError("reified:<Code>: …")` and
/// changes no state.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_hook_errors_are_typed() {
    let channel = gstring("rrs-p2-hooks");
    // Changed by P8 step 4 (design v2 AG-2, W4): the space of `sample_use` and the annotation
    // of `sample_new_space` are ordinary names, so the constructs translate
    // (`reified_hook_agent_constructs_translate`).
    // The space of `sample_use` and the annotation of `sample_new_space` are ordinary names.
    let cases = [
        // (sample_use(), AGENT_SPACES_UNAVAILABLE),
        // (sample_new_space(), AGENT_SPACES_UNAVAILABLE),
        (
            marker_tuple(Marker::Use, vec![gint(1)]),
            "reified:Malformed: the USE tuple has 2 elements instead of 3",
        ),
        (
            send(channel.clone(), vec![gint(1), sample_params()]),
            "reified:ParamsUnsupported: a send to this channel takes no params",
        ),
        (
            persistent_send(channel.clone(), vec![sample_params()]),
            "reified:ParamsUnsupported: a send to this channel takes no params",
        ),
        (
            send(channel.clone(), vec![marker_tuple(Marker::Params, vec![
                list(vec![]),
                map(vec![]),
            ])]),
            "reified:Malformed: the PARAMS tuple holds an erased empty form",
        ),
        (
            receive(sample_query(), vec![wildcard()], 0, Par::default(), false),
            "reified:QuerySpecUnsupported: a receive from this channel takes no query",
        ),
        (
            receive(
                marker_tuple(Marker::Query, vec![gint(1), list(vec![gint(2)])]),
                vec![wildcard()],
                0,
                Par::default(),
                false,
            ),
            "reified:Malformed: the QUERY tuple has 3 elements instead of 4",
        ),
    ];
    for (program, expected) in cases {
        let harness = harness(HashMap::new()).await;
        match harness.reducer.inj(program, rand()).await {
            Err(InterpreterError::ReduceError(message)) => assert_eq!(message, expected),
            other => panic!("expected {expected:?}, got {other:?}"),
        }
        let checkpoint = harness.space.create_soft_checkpoint().await;
        assert!(store_entries(&checkpoint.cache_snapshot).is_empty());
        assert!(checkpoint.log.is_empty());
    }
}

// ── Builders and the rholang helpers ───────────────────────────────────────

/// The models builders compute the fields of `prepend_expr`, `prepend_new` and the
/// collection normalizer.
#[test]
fn reified_builders_match_rholang_prepend_helpers() {
    let process_constructs = prop_oneof![
        reified_strategies::arb_use_block(arb_par()).prop_map(ReifiedConstruct::Use),
        reified_strategies::arb_new_space(arb_par()).prop_map(ReifiedConstruct::NewSpace),
    ];
    let mut runner = TestRunner::new(property_config());
    runner
        .run(
            &(process_constructs, arb_par(), 0i32..3),
            |(construct, par, depth)| {
                let tuple = tuple_of(&position_par(&construct));
                let pushed = push_process_expr(par.clone(), tuple.clone());
                let prepended = prepend_expr(par, tuple_expr(tuple.clone()), depth);
                prop_assert_eq!(pushed.encode_to_vec(), prepended.encode_to_vec());
                if let ReifiedConstruct::NewSpace(new_space) = &construct {
                    let expected = prepend_new(Par::default(), new_space.new.clone());
                    prop_assert_eq!(tuple.ps[2].encode_to_vec(), expected.encode_to_vec());
                }
                Ok(())
            },
        )
        .expect("push_process_expr and the New element follow the rholang helpers");

    // The collection normalizer: a tuple, a list and a map over a bound variable and a
    // connective. The builders give the same fields over the same elements.
    let normalized = Compiler::source_to_adt(
        r#"new x in { @"rrs-p2-normalizer"!((*x, Int), [*x, 2], {"k": *x}) }"#,
    )
    .expect("the source normalizes");
    let data = &normalized.news[0]
        .p
        .as_ref()
        .expect("the new has a body")
        .sends[0]
        .data;
    let normalized_tuple = tuple_of(&data[0]);
    let built = use_block(
        normalized_tuple.ps[0].clone(),
        normalized_tuple.ps[1].clone(),
    );
    assert_eq!(built.locally_free, normalized_tuple.locally_free);
    assert_eq!(built.connective_used, normalized_tuple.connective_used);
    assert!(built.connective_used, "Int is a connective");
    assert_eq!(built.locally_free, vec![1]);

    let elements = match &data[1].exprs[0].expr_instance {
        Some(ExprInstance::EListBody(elist)) => elist.ps.clone(),
        other => panic!("the second argument is a list, found {other:?}"),
    };
    let params = send_params(elements.into_iter().map(SendParam::Positional).collect())
        .expect("positional params build")
        .expect("params are not empty");
    let built_list = &tuple_of(&params).ps[1];
    assert_eq!(built_list.encode_to_vec(), data[1].encode_to_vec());

    let value = match &data[2].exprs[0].expr_instance {
        Some(ExprInstance::EMapBody(emap)) => emap.kvs[0]
            .value
            .clone()
            .expect("the map entry has a value"),
        other => panic!("the third argument is a map, found {other:?}"),
    };
    let params = send_params(vec![SendParam::Named("k".to_owned(), value)])
        .expect("named params build")
        .expect("params are not empty");
    let built_map = &tuple_of(&params).ps[2];
    assert_eq!(built_map.encode_to_vec(), data[2].encode_to_vec());
}

// ── Runtime ids ────────────────────────────────────────────────────────────

/// No name of the runtime is a marker: the URN map, every byte name, the mergeable tags and
/// fresh names of `new`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn marker_ids_disjoint_from_runtime_ids() {
    let mut stores = InMemoryStoreManager::new();
    let store = stores
        .r_space_stores()
        .await
        .expect("create the in-memory RSpace stores");
    let (play, _replay, _history) = create_runtimes(store, false, &mut Vec::new()).await;
    let urn_map = play.reducer.urn_map.clone();
    assert!(
        urn_map.contains_key("rho:io:stdout"),
        "the runtime binds its system processes"
    );
    for (urn, name) in urn_map.iter() {
        assert_eq!(marker_of(name), None, "{urn}");
        assert!(!contains_marker(name), "{urn}");
    }
    for byte in 0..=u8::MAX {
        assert_eq!(marker_of(&byte_name(byte)), None);
    }
    let tags = default_mergeable_tags();
    assert_eq!(tags.len(), 2);
    for tag in tags.keys().chain(play.reducer.mergeable_tags.keys()) {
        assert_eq!(marker_of(tag), None);
        assert!(!contains_marker(tag));
    }
    let mut random = rand();
    for _ in 0..64 {
        let id: Vec<u8> = random.next().into_iter().map(|byte| byte as u8).collect();
        assert_eq!(id.len(), 32);
        assert_eq!(Marker::from_id(&id), None);
    }
}

// ── Accounting twin ────────────────────────────────────────────────────────

/// The funding resolver keeps the HEAD filter, so it fails closed on a Par with a USE or
/// NEWSPACE tuple in a resolving position (finding H-1): at the top and in the body of a `new`.
/// A marker-free Par and an inert marker tuple resolve. The resolver does not resolve a receive
/// body (DR-121 of `feature/cost-accounted-rho`), so a construct in a receive body resolves.
#[test]
fn lexical_resolver_fails_closed_on_reified_pars() {
    let urn_map = HashMap::new();
    let ordinary = concatenate_pars(
        send(gstring("rrs-p2-lexical"), vec![gint(1)]),
        concatenate_pars(bound_var(0), tuple(vec![gint(1), gint(2)])),
    );
    assert!(resolve_lexical_names_for_funding(&ordinary, rand(), &urn_map).is_ok());
    let inert = concatenate_pars(ordinary.clone(), sample_params());
    assert!(resolve_lexical_names_for_funding(&inert, rand(), &urn_map).is_ok());
    let disagreement = Err(InterpreterError::BugFoundError(
        LEXICAL_DISAGREEMENT.to_owned(),
    ));
    for marked in [sample_use(), sample_new_space()] {
        let top = concatenate_pars(ordinary.clone(), marked.clone());
        assert_eq!(
            resolve_lexical_names_for_funding(&top, rand(), &urn_map),
            disagreement
        );
        // Changed by decision 89 (DR-121): the resolver leaves a receive body unresolved, so
        // the nested case that fails closed is the body of a `new`, a resolving position.
        // let nested = receive(
        //     gstring("rrs-p2-lexical"),
        //     vec![wildcard()],
        //     0,
        //     marked,
        //     false,
        // );
        // assert_eq!(
        //     resolve_lexical_names_for_funding(&nested, rand(), &urn_map),
        //     disagreement
        // );
        let nested = new_par(1, marked.clone());
        assert_eq!(
            resolve_lexical_names_for_funding(&nested, rand(), &urn_map),
            disagreement
        );
        let received = receive(
            gstring("rrs-p2-lexical"),
            vec![wildcard()],
            0,
            marked,
            false,
        );
        assert!(resolve_lexical_names_for_funding(&received, rand(), &urn_map).is_ok());
    }
}

/// P8 step 4 (design v2 AG-2, TS-1, W4): the space of `sample_use` and the annotation of
/// `sample_new_space` are ordinary names, so both constructs translate without an error. The
/// USE body sends on the forged channel `1`, which becomes one call on the agent. The NEWSPACE
/// tuple makes one gensym call and parks its resume term.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_hook_agent_constructs_translate() {
    let outcome = evaluate(sample_use()).await;
    assert_eq!(outcome.error, None);
    assert_eq!(
        (outcome.produces, outcome.consumes, outcome.comms),
        (1, 0, 0)
    );
    assert_eq!(data_channels(&outcome), vec![
        private_name(vec![3; 32]).encode_to_vec()
    ]);
    let outcome = evaluate(sample_new_space()).await;
    assert_eq!(outcome.error, None);
    assert_eq!(
        (outcome.produces, outcome.consumes, outcome.comms),
        (1, 1, 0)
    );
    assert_eq!(data_channels(&outcome), vec![
        private_name(vec![4; 32]).encode_to_vec()
    ]);
}

// ── HEAD empty-state inputs ────────────────────────────────────────────────

/// The inputs of both HEAD empty-state tests hold no marker: the registry bootstrap term and
/// the contract of the second test. Their term schedules are the HEAD schedules.
#[test]
fn head_empty_state_inputs_marker_free() {
    let bootstrap = registry_bootstrap::ast();
    let contract = Compiler::source_to_adt(RUNTIME_SPEC_CONTRACT)
        .expect("the runtime_spec contract normalizes");
    for (name, term) in [
        ("registry bootstrap", &bootstrap),
        ("runtime_spec contract", &contract),
    ] {
        assert!(!contains_marker(term), "{name} holds a marker id");
        assert_eq!(
            term_keys(&evaluation_terms(term)),
            term_keys(&head_evaluation_terms(term)),
            "{name}"
        );
        for expr in &term.exprs {
            assert_eq!(is_evaluated_process_expr(expr), head_keeps(expr), "{name}");
        }
    }
    assert_eq!(bootstrap.news.len(), 5, "the bootstrap term is five news");
}

// ── Consensus checklist items 18 and 19 ────────────────────────────────────

/// Item 18 at P2: every marker form fails closed before it creates a consume, so the hooks
/// add no join. An ordinary join is the control: it parks one consume.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_p2_synthesizes_no_join() {
    let left = gstring("rrs-p2-join-left");
    let query = sample_query();
    let malformed_query = marker_tuple(Marker::Query, vec![gint(1), list(vec![gint(2)])]);
    let programs = [
        // Changed by P8 step 4 (design v2 AG-2, W4): these constructs name user agents, so they
        // translate instead of failing closed (checked below).
        // sample_use(),
        // sample_new_space(),
        marker_tuple(Marker::Use, vec![gint(1)]),
        // new_par(1, sample_use()),
        send(left.clone(), vec![gint(1), sample_params()]),
        persistent_send(left.clone(), vec![sample_params()]),
        receive(query.clone(), vec![wildcard()], 0, Par::default(), false),
        receive(query.clone(), vec![wildcard()], 0, Par::default(), true),
        join(
            vec![
                (left.clone(), vec![wildcard()], 0),
                (query.clone(), vec![wildcard()], 0),
            ],
            Par::default(),
            false,
            false,
        ),
        join(
            vec![
                (query.clone(), vec![wildcard()], 0),
                (left.clone(), vec![wildcard()], 0),
            ],
            Par::default(),
            false,
            true,
        ),
        receive(malformed_query, vec![wildcard()], 0, Par::default(), false),
    ];
    for program in programs {
        let outcome = evaluate(program.clone()).await;
        assert!(
            outcome.code().is_some(),
            "the program fails closed: {outcome:?}"
        );
        assert!(outcome.is_quiet(), "{outcome:?}");
    }
    // P8 step 4: the constructs of user agents translate with single-channel receives (I-16).
    for program in [sample_use(), sample_new_space(), new_par(1, sample_use())] {
        let outcome = evaluate(program).await;
        assert_eq!(outcome.error, None, "{outcome:?}");
        assert!(outcome.widest_join < 2, "{outcome:?}");
    }
    let control = join(
        vec![
            (left.clone(), vec![wildcard()], 0),
            (gstring("rrs-p2-join-right"), vec![wildcard()], 0),
        ],
        Par::default(),
        false,
        false,
    );
    let outcome = evaluate(control).await;
    assert_eq!(outcome.error, None);
    assert_eq!(outcome.consumes, 1);
    assert!(!outcome.store.is_empty());
    assert_eq!(outcome.widest_join, 2);
}

/// Item 19 at P2: neither a marker tuple nor a 0xE0 name is a mergeable channel. A tuple
/// headed by a registered mergeable tag is the control.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_p2_marker_channels_never_mergeable() {
    let mut channels: Vec<Par> = vec![
        sample_use(),
        sample_new_space(),
        sample_params(),
        sample_query(),
        marker_tuple(Marker::Free, vec![gstring("Nat"), list(vec![])]),
        tuple(vec![Marker::Params.par(), gint(1)]),
        private_name(vec![0xE0, 0x21]),
        private_name(vec![0xE0, 0x30]),
        tuple(vec![
            private_name([vec![0xE0, 0x44], vec![9; 32]].concat()),
            gint(1),
        ]),
    ];
    channels.extend(Marker::ALL.map(Marker::par));
    let control = tuple(vec![non_negative_mergeable_tag_name(), gint(1)]);
    let mut program = send(control.clone(), vec![gint(5)]);
    for (index, channel) in channels.iter().enumerate() {
        program = concatenate_pars(send(channel.clone(), vec![gint(5)]), program);
        if index % 2 == 0 && channel != &sample_query() {
            program = concatenate_pars(
                receive(
                    channel.clone(),
                    vec![wildcard(), wildcard()],
                    0,
                    Par::default(),
                    false,
                ),
                program,
            );
        }
    }
    let (outcome, mergeable) = evaluate_with(program, default_mergeable_tags()).await;
    assert_eq!(outcome.error, None);
    let keys: BTreeSet<Vec<u8>> = mergeable
        .keys()
        .map(|channel| channel.encode_to_vec())
        .collect();
    assert_eq!(keys, BTreeSet::from([control.encode_to_vec()]));
    assert_eq!(mergeable.get(&control), Some(&MergeType::IntegerAdd));
    for channel in &channels {
        assert!(!mergeable.contains_key(channel), "{channel:?}");
    }
}
