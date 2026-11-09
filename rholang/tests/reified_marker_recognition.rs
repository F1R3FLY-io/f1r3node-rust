// P2 tests of the Reified RSpaces recognition hooks (plan decision 22), with the P4 codes.
//
// The property tests have the names that formal/reified-rspaces-invariant-map.json maps to
// the headlines E7-E12 of formal/rocq/reified_rspaces/theories/Encoding.v. Each fixture is a
// Par that the models builders build, and the reducer evaluates it with `inj`. Every
// recognized construct fails closed with a typed `reified:<Code>:` error, or it translates. No
// space reference of a fixture names a native space (P4 design C.2 and C.3). The empty Par
// gives `EmptySpaceRef`. Every other value names a user agent, and from P8 step 4 the construct
// Changed by decision 112: the Reified syntax lives in protobuf fields, so no hook refreshes a flag
// and an annotated `new` replaces the NEWSPACE tuple. The text before:
// ```
// // translates: a USE block runs its body in the context of the agent, and a NEWSPACE tuple with
// // an agent annotation makes a gensym call and parks its resume term (`hook_outcome`). Before
// // the strict decode, the hooks recompute the flags that the encoder derives (I-20). Two tests
// // check consensus checklist items 18 and 19 for P2.
// ```
// translates: a USE block runs its body in the context of the agent, and an annotated `new` with
// an agent annotation makes a gensym call and parks its resume term (`hook_outcome`). The
// Reified syntax lives in protobuf fields (decision 112), so a tuple with a former marker head
// is an ordinary tuple. Two tests check consensus checklist items 18 and 19 for P2.

#[path = "../../models/tests/support/reified_strategies.rs"]
mod reified_strategies;

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::{VarInstance, WildcardMsg};
// Changed by decision 112: the tests read the protobuf fields through the field decoders.
// The text before:
// ```
// use models::rhoapi::{
//     BindPattern, Bundle, EEq, EMethod, ETuple, EVar, Expr, If, ListParWithRandom, Match, MatchCase,
//     New, Par, Receive, ReceiveBind, TaggedContinuation, Var,
// };
// use models::rust::reified::builders::{push_process_expr, send_params, use_block};
// use models::rust::reified::encoding::{
//     decode_free_value, decode_process_tuple, decode_query_source, encode, encode_new_space,
//     marker_of, split_trailing_params, Marker, NewSpace, ReifiedConstruct, ReifiedDecodeError,
// };
// ```
use models::rhoapi::{
    BindPattern, Bundle, EEq, EMethod, ETuple, EVar, Expr, If, ListParWithRandom, Match, MatchCase,
    New, Par, QuerySpec, Receive, ReceiveBind, SendParams, TaggedContinuation, Var,
};
use models::rust::reified::builders::{push_use_block, query_spec, send_params, use_block};
use models::rust::reified::encoding::{
    check_query_spec, decode_free_value, decode_new_space, decode_use_block, split_trailing_params,
    Marker, NewSpace, ReifiedConstruct,
};
use models::rust::reified::params::SendParam;
use models::rust::rholang::implicits::concatenate_pars;
use models::rust::utils::union;
use proptest::prelude::*;
use proptest::sample::select;
use proptest::test_runner::{RngSeed, TestRunner};
use prost::Message;
// Changed by decision 112: the restated tests no longer read `int_connective`. The text before:
// ```
// use reified_strategies::{
//     arb_construct, arb_ground_construct, arb_ground_par, arb_named, arb_par, bound_var,
//     contains_marker, gbool, gint, gstring, int_connective, list, map, new_term, position_par,
//     private_name, send, sole_tuple, tuple,
// };
// ```
use reified_strategies::{
    arb_construct, arb_ground_construct, arb_ground_par, arb_named, arb_par, bound_var,
    contains_marker, gbool, gint, gstring, list, map, new_term, position_par, private_name, send,
    sole_tuple, tuple,
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
// Disabled by decision 90 (R-1): no test expects the resolver to fail closed on a marker now.
// const LEXICAL_DISAGREEMENT: &str = "funding resolver term schedule disagrees with reducer schedule";
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
        // Changed by Reified RSpaces (user decision of 2026-10-25, as DR-128): a user failure of
        // a space is a `SpaceUserError` with the text of its reduce error.
        // InterpreterError::ReduceError(message) => message.clone(),
        InterpreterError::ReduceError(message) | InterpreterError::SpaceUserError(message) => {
            message.clone()
        }
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
            query_spec: None,
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

/// `par` with `params` in the `params` field of its first send.
fn with_params(mut par: Par, params: SendParams) -> Par {
    par.sends[0].params = Some(params);
    par
}

/// `par` with `query` in the `query_spec` field of the bind `index` of its first receive.
fn with_query(mut par: Par, index: usize, query: QuerySpec) -> Par {
    par.receives[0].binds[index].query_spec = Some(query);
    par
}

/// A use block without a body, which no frontend builds.
fn malformed_use_block() -> Par {
    Par {
        use_blocks: vec![models::rhoapi::UseBlock {
            space: Some(gint(1)),
            body: None,
        }],
        ..Par::default()
    }
}

fn new_par(bind_count: i32, body: Par) -> Par {
    Par {
        news: vec![New {
            bind_count,
            p: Some(body),
            uri: Vec::new(),
            injections: Default::default(),
            locally_free: Vec::new(),
            space_types: Vec::new(),
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

// Changed by decision 112: the tuple builder retires, so `prepend_expr` adds the tuple.
// The text before:
// fn process(tuple: ETuple) -> Par { push_process_expr(Par::default(), tuple) }
fn process(tuple: ETuple) -> Par { prepend_expr(Par::default(), tuple_expr(tuple), 0) }

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

// Changed by decision 112: the samples are the field forms and the values that the hooks read.
// The text before:
// ```
// fn sample_use() -> Par {
//     marker_tuple(Marker::Use, vec![
//         private_name(vec![3; 32]),
//         send(gint(1), vec![gint(2)]),
//     ])
// }
//
// fn sample_new_space() -> Par {
//     process(
//         match encode_new_space(&NewSpace {
//             annotations: vec![Some(private_name(vec![4; 32]))],
//             new: new_term(1, Vec::new(), send(gint(1), vec![gint(2)])),
//         })
//         .expect("an annotated new encodes")
//         .expr_instance
//         {
//             Some(ExprInstance::ETupleBody(tuple)) => tuple,
//             other => panic!("encode_new_space returned {other:?}"),
//         },
//     )
// }
//
// fn sample_params() -> Par {
//     send_params(vec![
//         SendParam::Positional(gint(3)),
//         SendParam::Named("priority".to_owned(), gint(4)),
//     ])
//     .expect("params build")
//     .expect("params are not empty")
// }
//
// fn sample_query() -> Par {
//     marker_tuple(Marker::Query, vec![
//         gstring("rrs-p2-query"),
//         list(vec![gint(5)]),
//         map(vec![("k".to_owned(), gint(1))]),
//     ])
// }
// ```
fn sample_use() -> Par {
    position_par(&ReifiedConstruct::Use(
        models::rust::reified::encoding::UseBlock {
            space: private_name(vec![3; 32]),
            body: send(gint(1), vec![gint(2)]),
        },
    ))
}

fn sample_new_space() -> Par {
    position_par(&ReifiedConstruct::NewSpace(NewSpace {
        annotations: vec![Some(private_name(vec![4; 32]))],
        new: new_term(1, Vec::new(), send(gint(1), vec![gint(2)])),
    }))
}

fn sample_params() -> SendParams {
    send_params(vec![
        SendParam::Positional(gint(3)),
        SendParam::Named("priority".to_owned(), gint(4)),
    ])
    .expect("params build")
    .expect("params are not empty")
}

fn sample_query() -> QuerySpec {
    query_spec(vec![gint(5)], vec![SendParam::Named(
        "k".to_owned(),
        gint(1),
    )])
    .expect("a query builds")
}

/// True when `par` is the name of a former marker.
fn is_marker_name(par: &Par) -> bool { Marker::ALL.into_iter().any(|marker| marker.par() == *par) }

/// The tuple that encoded `construct` before decision 112. Its head is the former marker id,
/// which is an ordinary private name now.
fn former_encoding(construct: &ReifiedConstruct) -> ETuple {
    let entries = |named: &std::collections::BTreeMap<String, Par>| -> Vec<(String, Par)> {
        named
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()
    };
    let fields = match construct {
        ReifiedConstruct::Use(block) => vec![block.space.clone(), block.body.clone()],
        ReifiedConstruct::NewSpace(new_space) => vec![
            list(
                new_space
                    .annotations
                    .iter()
                    .map(|annotation| list(annotation.iter().cloned().collect()))
                    .collect(),
            ),
            prepend_new(Par::default(), new_space.new.clone()),
        ],
        ReifiedConstruct::Params(params) => {
            vec![list(params.positional.clone()), map(entries(&params.named))]
        }
        ReifiedConstruct::Query(query) => vec![
            gstring("rrs-p2-e8-query"),
            list(query.payload.clone()),
            map(entries(&query.named)),
        ],
        ReifiedConstruct::Free(theory) => {
            vec![gstring(&theory.name), list(theory.args.clone())]
        }
    };
    tuple_of(&marker_tuple(construct.marker(), fields))
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
            // Changed by decision 112: the field decoders read the construct. The text before:
            // ```
            // prop_assert_eq!(
            //     decode_process_tuple(&tuple_of(&par)).map(ReifiedConstruct::from),
            //     Ok(construct.clone())
            // );
            // ```
            prop_assert_eq!(decode_in_position(&par, marker), Ok(construct.clone()));
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
                // Changed by decision 112: a send holds its params in the field. The text before:
                // ```
                // send(channel.clone(), vec![gint(1), par.clone()]),
                // persistent_send(channel.clone(), vec![par.clone()]),
                // ```
                with_params(send(channel.clone(), vec![gint(1)]), params.clone()),
                with_params(persistent_send(channel.clone(), Vec::new()), params.clone()),
            ] {
                let outcome = evaluate(program).await;
                prop_assert_eq!(outcome.code(), Some("ParamsUnsupported"));
                prop_assert!(outcome.is_quiet());
            }
        }
        ReifiedConstruct::Query(query) => {
            // Changed by decision 112: a bind holds its query in the field. The text before:
            // ```
            // prop_assert_eq!(decode_query_source(&par), Ok(Some(query.clone())));
            // let outcome = evaluate(receive(
            //     par.clone(),
            //     vec![wildcard()],
            //     0,
            //     Par::default(),
            //     false,
            // ))
            // .await;
            // ```
            prop_assert_eq!(decode_in_position(&par, marker), Ok(construct.clone()));
            let outcome = evaluate(with_query(
                receive(channel.clone(), vec![wildcard()], 0, Par::default(), false),
                0,
                query.clone(),
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
    // Changed by decision 112: a receive with a query is a process that the reducer evaluates, so
    // the inert values are the two value forms. The text before:
    // if !matches!(marker, Marker::Use | Marker::NewSpace) {
    if matches!(marker, Marker::Params | Marker::Free) {
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

// Changed by decision 112: the tuples of this test are former encodings. The text before:
// ```
// /// A tuple with a marker head: the canonical encoding of a construct, random fields of any
// /// arity, or a canonical encoding with a flipped flag.
// ```
/// A tuple with a former marker head: the former encoding of a construct, random fields of any
/// arity, or a former encoding with a flipped flag.
fn arb_forged() -> BoxedStrategy<ETuple> {
    // Changed by decision 112: `position_par` builds the field forms, so the former encoding has
    // its own helper. The text before:
    // ```
    // let canonical =
    //     arb_ground_construct().prop_map(|construct| tuple_of(&position_par(&construct)));
    // ```
    let canonical = arb_ground_construct().prop_map(|construct| former_encoding(&construct));
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

// Changed by decision 112: no tuple is a Reified form, so a forged tuple grants nothing.
// The text before:
// ```
// /// E8 (marker_forging_grants_no_capability): a term that a hook recognizes is the canonical
// /// encoding of a well-formed construct that the position allows. The hook refreshes the
// /// derived flags of the term first (I-20). So a forged term behaves as the syntax that it
// /// equals. A malformed marker term fails closed.
// ```
/// E8, restated by decision 112: a tuple with a former marker head is an ordinary tuple. It is
/// inert in process position, it is ordinary data in the last data position of a send, it is an
/// ordinary channel as a bind source, and the value decoders read no construct from it. So a
/// forged tuple grants no capability, whatever its flags are.
#[test]
// Changed by decision 112: the test covers a former marker tuple. The text before:
// fn marker_forging_grants_no_capability() {
fn former_marker_tuple_is_ordinary() {
    let runtime = runtime();
    let mut runner = TestRunner::new(property_config());
    runner
        .run(&arb_forged(), |forged| {
            runtime.block_on(check_forged(forged))
        })
        // Changed by decision 112: a former marker tuple is ordinary. The text before:
        // .expect("forged marker tuples grant no capability");
        .expect("a tuple with a former marker head is an ordinary tuple");
}

// Changed by decision 112: no hook recognizes a tuple, so every position treats a forged tuple as
// an ordinary tuple. The text before:
// ```
// async fn check_forged(forged: ETuple) -> Result<(), TestCaseError> {
//     let marker = marker_of(&forged.ps[0]).expect("a forged tuple has a marker head");
//     let forged_par = process(forged.clone());
//     // I-20: before the strict decode, a hook recomputes the flags that the encoder derives.
//     // The fields of a forged tuple carry their derived flags, so only the flag of the tuple
//     // can be stale. `tuple` recomputes that flag from the fields. The decoders read a tuple of
//     // another marker as no construct, whatever its flags are.
//     let refreshed = tuple_of(&tuple(forged.ps.clone()));
//     prop_assert_eq!(&refreshed.ps, &forged.ps);
//     let refreshed_par = process(refreshed.clone());
//     let channel = gstring("rrs-p2-e8");
//
//     // Process position (H1 and H2).
//     let outcome = evaluate(forged_par.clone()).await;
//     match (marker, decode_process_tuple(&refreshed)) {
//         (Marker::Use | Marker::NewSpace, Ok(construct)) => {
//             let construct = ReifiedConstruct::from(construct);
//             prop_assert_eq!(construct.marker(), marker);
//             let canonical =
//                 encode(&construct).map_err(|error| TestCaseError::fail(error.to_string()))?;
//             prop_assert_eq!(&canonical, &tuple_expr(refreshed.clone()));
//             // Changed by P8 step 4 (design v2 AG-2, W4): see `hook_outcome`.
//             // prop_assert_eq!(outcome.error.as_deref(), space_ref_error(&construct));
//             match hook_outcome(&construct) {
//                 Some(expected) => check_hook_outcome(&outcome, &expected)?,
//                 None => prop_assert_eq!(&outcome.error, &None),
//             }
//             prop_assert_eq!(&outcome, &evaluate(position_par(&construct)).await);
//         }
//         (Marker::Use | Marker::NewSpace, Err(_)) => {
//             prop_assert_eq!(outcome.code(), Some("Malformed"));
//             prop_assert!(outcome.is_quiet());
//         }
//         (Marker::Params | Marker::Query | Marker::Free, _) => {
//             prop_assert_eq!(&outcome.error, &None);
//             prop_assert!(outcome.is_quiet());
//         }
//     }
//
//     // The last data element of a send (H3).
//     let outcome = evaluate(send(channel.clone(), vec![forged_par.clone()])).await;
//     match split_trailing_params(std::slice::from_ref(&refreshed_par)) {
//         Ok(Some(params)) => {
//             let construct = ReifiedConstruct::Params(params);
//             prop_assert_eq!(&position_par(&construct), &refreshed_par);
//             prop_assert_eq!(outcome.code(), Some("ParamsUnsupported"));
//             let replayed = evaluate(send(channel.clone(), vec![position_par(&construct)])).await;
//             prop_assert_eq!(&outcome, &replayed);
//         }
//         Err(_) => prop_assert_eq!(outcome.code(), Some("Malformed")),
//         Ok(None) => {
//             prop_assert_eq!(&outcome.error, &None);
//             prop_assert_eq!(outcome.produces, 1);
//         }
//     }
//
//     // The source of a receive bind (H4).
//     let outcome = evaluate(receive(
//         forged_par.clone(),
//         vec![wildcard()],
//         0,
//         Par::default(),
//         false,
//     ))
//     .await;
//     match decode_query_source(&refreshed_par) {
//         Ok(Some(query)) => {
//             let construct = ReifiedConstruct::Query(query);
//             prop_assert_eq!(&position_par(&construct), &refreshed_par);
//             prop_assert_eq!(outcome.code(), Some("QuerySpecUnsupported"));
//             let replayed = evaluate(receive(
//                 position_par(&construct),
//                 vec![wildcard()],
//                 0,
//                 Par::default(),
//                 false,
//             ))
//             .await;
//             prop_assert_eq!(&outcome, &replayed);
//         }
//         Err(_) => prop_assert_eq!(outcome.code(), Some("Malformed")),
//         Ok(None) => {
//             prop_assert_eq!(&outcome.error, &None);
//             prop_assert_eq!(outcome.consumes, 1);
//         }
//     }
//     Ok(())
// }
// ```
async fn check_forged(forged: ETuple) -> Result<(), TestCaseError> {
    prop_assert!(
        is_marker_name(&forged.ps[0]),
        "a forged tuple has a former marker head"
    );
    let forged_par = process(forged);
    let channel = gstring("rrs-p2-e8");

    // Process position: a tuple is no term of the schedule, so nothing runs.
    let outcome = evaluate(forged_par.clone()).await;
    prop_assert_eq!(&outcome.error, &None);
    prop_assert!(outcome.is_quiet());

    // The last data element of a send is ordinary data.
    let outcome = evaluate(send(channel.clone(), vec![forged_par.clone()])).await;
    prop_assert_eq!(&outcome.error, &None);
    prop_assert_eq!(outcome.produces, 1);

    // The source of a receive bind is an ordinary channel.
    let outcome = evaluate(receive(
        forged_par.clone(),
        vec![wildcard()],
        0,
        Par::default(),
        false,
    ))
    .await;
    prop_assert_eq!(&outcome.error, &None);
    prop_assert_eq!(outcome.consumes, 1);

    // The value decoders read no construct from a tuple.
    prop_assert_eq!(decode_free_value(&forged_par), Ok(None));
    prop_assert_eq!(
        split_trailing_params(std::slice::from_ref(&forged_par)),
        Ok(None)
    );
    Ok(())
}

// ── E9 ─────────────────────────────────────────────────────────────────────

// Retired by decision 112: a field holds no marker id, so the decoy comparison has no subject.
// The text before:
// ```
// /// The unused id `[0xE0, m + 5]` that stands in for the marker `m`.
// fn decoy_index(marker: Marker) -> u8 { marker as u8 + 5 }
//
// /// Replaces every `GPrivate` id `[0xE0, m + 5]` by `[0xE0, m]` in prost bytes.
// fn rename_decoys(bytes: &[u8]) -> Vec<u8> {
//     let mut renamed = bytes.to_vec();
//     for marker in Marker::ALL {
//         let decoy = [0x0a, 0x02, 0xE0, decoy_index(marker)];
//         let mut index = 0;
//         while index + decoy.len() <= renamed.len() {
//             if renamed[index..index + decoy.len()] == decoy {
//                 renamed[index + 3] = marker as u8;
//                 index += decoy.len();
//             } else {
//                 index += 1;
//             }
//         }
//     }
//     renamed
// }
// ```

// Changed by decision 112: `marked` is the field form of a construct. The text before:
// /// A program with the tuple `marked` in the inert positions that `mask` selects.
/// A program with the field form `marked` in the inert positions that `mask` selects.
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
    // Changed by decision 112: a use block, an annotated `new` and a receive with a query are
    // processes, so the inert values are the two value forms. The text before:
    // ```
    // if chosen(8) && !matches!(marker, Marker::Use | Marker::NewSpace) {
    //     // Process position, for a marker that the reducer does not evaluate there.
    // ```
    if chosen(8) && matches!(marker, Marker::Params | Marker::Free) {
        // Process position, for a value form that is no term of the schedule.
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

// Changed by decision 112: the fields replace the tuples and no decoy id exists. The text before:
// ```
// /// E9 (marker_inert_elsewhere): a marker tuple outside its recognized position is ordinary
// /// data. A program behaves the same, up to renaming, when an unused 2-byte decoy id replaces
// /// the marker id.
// ```
/// E9, restated by decision 112: a field outside its process position is ordinary data. A
/// program behaves the same when a string literal replaces the field: no error, and the same
/// numbers of produces, consumes, comms and store entries.
#[test]
// Changed by decision 112: the test covers the fields. The text before:
// fn marker_inert_elsewhere() {
fn fields_inert_outside_process_position() {
    let runtime = runtime();
    let mut runner = TestRunner::new(property_config());
    runner
        .run(
            &(arb_ground_construct(), 1u16..(1 << 11)),
            |(construct, mask)| runtime.block_on(check_inert(&construct, mask)),
        )
        // Changed by decision 112: the test covers the fields. The text before:
        // .expect("marker tuples are inert outside their positions");
        .expect("the fields are inert outside their process positions");
}

async fn check_inert(construct: &ReifiedConstruct, mask: u16) -> Result<(), TestCaseError> {
    let marker = construct.marker();
    let marked = position_par(construct);
    // Changed by decision 112: a string literal stands in for the field form. The text before:
    // ```
    // let mut decoy_tuple = tuple_of(&marked);
    // decoy_tuple.ps[0] = private_name(vec![0xE0, decoy_index(marker)]);
    // prop_assert_eq!(marker_of(&decoy_tuple.ps[0]), None);
    // let decoy = process(decoy_tuple);
    // ```
    let decoy = gstring("rrs-p2-e9-stand-in");

    let real = evaluate(inert_program(&marked, marker, mask)).await;
    let stand_in = evaluate(inert_program(&decoy, marker, mask)).await;
    prop_assert_eq!(&real.error, &None);
    prop_assert_eq!(&stand_in.error, &None);
    // Changed by decision 112: the stand-in differs from the field in its bytes, so the test
    // compares the counts of the two runs. The text before:
    // ```
    // let mut renamed: Vec<Vec<u8>> = stand_in
    //     .store
    //     .iter()
    //     .map(|entry| rename_decoys(entry))
    //     .collect();
    // renamed.sort();
    // prop_assert_eq!(&real.store, &renamed);
    // prop_assert_eq!(
    //     (real.produces, real.consumes, real.comms),
    //     (stand_in.produces, stand_in.consumes, stand_in.comms)
    // );
    // ```
    prop_assert_eq!(
        (real.produces, real.consumes, real.comms, real.store.len()),
        (
            stand_in.produces,
            stand_in.consumes,
            stand_in.comms,
            stand_in.store.len()
        )
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
            GeneratedMessage::UseBlock(message) => (9, message.encode_to_vec()),
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

// Changed by decision 112: the schedule counts use blocks, and an annotated `new` is an ordinary
// term of `news`. The text before:
// ```
// /// E10 (schedule_unchanged_marker_free): for a Par without a marker, both filter twins keep
// /// the terms of HEAD, so the term list, the term count and the random splits stay as at
// /// HEAD. A Par with k USE or NEWSPACE tuples gets exactly k more terms, and the funding
// /// resolver, which keeps the HEAD filter, then fails closed.
// ```
/// E10, restated by decision 112: for a Par without a Reified field, both filter twins keep the
/// terms of HEAD, so the term list, the term count and the random splits stay as at HEAD. Each
/// use block adds one term, and an annotated `new` takes its place among the `new` terms. The
/// funding resolver follows the schedule of the reducer and keeps each field as it was.
#[test]
// Changed by decision 112: the test covers the fields. The text before:
// fn term_schedule_unchanged_for_marker_free_pars() {
fn term_schedule_unchanged_for_field_free_pars() {
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
                // Changed by decision 112: a field joins the Par as its own process.
                // The text before:
                // push_process_expr(marked, tuple_of(&position_par(construct)))
                concatenate_pars(position_par(construct), marked)
            });
            let marked_terms = evaluation_terms(&marked).len();
            prop_assert_eq!(
                marked_terms,
                // Changed by decision 112: only a use block is a new term of the schedule.
                // The text before:
                // head_evaluation_terms(&marked).len() + markers.len()
                head_evaluation_terms(&marked).len() + marked.use_blocks.len()
            );
            prop_assert_eq!(
                term_keys(&owned_evaluation_terms(marked.clone())).len(),
                marked_terms
            );
            let resolved = resolve_lexical_names_for_funding(&marked, rand(), &HashMap::new());
            // Changed by decision 90 (R-1): the resolver counts a marker tuple with the
            // predicate of the reducer, so a marked Par resolves, and the resolver never walks a
            // marker (R-2), so each marker tuple stays as it was.
            // match markers.is_empty() {
            //     true => prop_assert!(resolved.is_ok()),
            //     false => prop_assert_eq!(
            //         resolved,
            //         Err(InterpreterError::BugFoundError(
            //             LEXICAL_DISAGREEMENT.to_owned()
            //         ))
            //     ),
            // }
            let resolved = resolved.expect("a marked Par resolves (R-1)");
            for construct in &markers {
                let field = position_par(construct);
                prop_assert!(
                    field
                        .use_blocks
                        .iter()
                        .all(|block| resolved.use_blocks.contains(block))
                        && field.news.iter().all(|new| resolved.news.contains(new)),
                    "a field changed"
                );
            }
            Ok(())
        })
        // Changed by decision 112: the test covers the fields. The text before:
        // .expect("marker-free Pars keep the HEAD term schedule");
        .expect("field-free Pars keep the HEAD term schedule");
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
                // Changed by decision 112: a query travels on its bind, so it holds no channel.
                // The text before:
                // channel: substitute(&query.channel, env, variant),
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
        // Changed by decision 112: the decoders read the use block and the annotated `new`.
        // The text before:
        // ```
        // Marker::Use | Marker::NewSpace => decode_process_tuple(&tuple_of(par))
        //     .map(|construct| Some(ReifiedConstruct::from(construct))),
        // ```
        Marker::Use => {
            decode_use_block(&par.use_blocks[0]).map(|block| Some(ReifiedConstruct::Use(block)))
        }
        Marker::NewSpace => decode_new_space(&par.news[0])
            .map(|new_space| Some(ReifiedConstruct::NewSpace(new_space))),
        Marker::Params => split_trailing_params(std::slice::from_ref(par))
            .map(|params| params.map(ReifiedConstruct::Params)),
        // Changed by decision 112: the query is the field of the one bind of the receive.
        // The text before:
        // ```
        // Marker::Query => decode_query_source(par).map(|query| query.map(ReifiedConstruct::Query)),
        // ```
        Marker::Query => {
            let query = par.receives[0].binds[0]
                .query_spec
                .clone()
                .expect("the bind holds a query");
            check_query_spec(&query).map(|()| Some(ReifiedConstruct::Query(query)))
        }
        Marker::Free => decode_free_value(par).map(|theory| theory.map(ReifiedConstruct::Free)),
    };
    match decoded {
        Ok(Some(construct)) => Ok(construct),
        other => Err(format!("{other:?}")),
    }
}

// Changed by decision 112: the fields hold no tuple flag, and no tuple is recognized.
// The text before:
// ```
// /// E11 (decode_commutes_with_subst): substitution maps the encoding of a construct to the
// /// encoding of the substituted construct. Substitution keeps the `connective_used` flag of a
// /// tuple, so the property holds for values without connectives. A connective value leaves the
// /// flag stale, and the decoder then fails closed. Recognition in process position depends on
// /// the substitution time (finding H-3).
// ```
/// E11, restated by decision 112: substitution maps each field of a construct to the field of
/// the substituted construct. A field has no flag that a substitution can leave stale, and no
/// substitution of a tuple changes what the reducer recognizes.
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

    // Retired by decision 112: the fields hold no tuple flag, so substitution leaves nothing stale.
    // The text before:
    // ```
    // // A connective value leaves the tuple flag stale: the decoder fails closed.
    // let with_variable = marker_tuple(Marker::Use, vec![gint(1), bound_var(0)]);
    // let env = Env::new().put(int_connective());
    // let stale = substitute(&with_variable, &env, Substitution::Sorted);
    // assert!(!tuple_of(&stale).connective_used);
    // assert!(tuple_of(&stale).ps[2].connective_used);
    // assert_eq!(
    //     decode_process_tuple(&tuple_of(&stale)),
    //     Err(ReifiedDecodeError::NonCanonical {
    //         marker: Marker::Use
    //     })
    // );
    // ```

    // Changed by decision 112: the reducer recognizes no tuple. The text before:
    // ```
    // // H-3: a tuple with a variable head is inert when the reducer evaluates it directly, and
    // // recognized once the storage substitution of a receive body makes the head literal.
    // ```
    // A tuple with a variable head is inert when the reducer evaluates it directly, and it stays
    // inert after the storage substitution of a receive body makes the head a former marker.
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
    // Changed by decision 112: the tuple with the former USE marker head is ordinary, so only the
    // two receives run and the name of the space stays empty. The text before:
    // ```
    // assert_eq!(
    //     (outcome.produces, outcome.consumes, outcome.comms),
    //     (3, 2, 2)
    // );
    // assert!(data_channels(&outcome).contains(&private_name(vec![6; 32]).encode_to_vec()));
    // ```
    assert_eq!(
        (outcome.produces, outcome.consumes, outcome.comms),
        (2, 2, 2)
    );
    assert!(!data_channels(&outcome).contains(&private_name(vec![6; 32]).encode_to_vec()));
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

// Retired by decision 112: no NEWSPACE tuple form exists, so the superseded form of the plan has no
// term. The text before:
// ```
// /// The superseded "s_i or Nil" NEWSPACE form of the plan.
// fn plan_new_space(annotations: &[Option<Par>], new: &New) -> Par {
//     marker_tuple(Marker::NewSpace, vec![
//         list(
//             annotations
//                 .iter()
//                 .map(|annotation| annotation.clone().unwrap_or_default())
//                 .collect(),
//         ),
//         Par {
//             news: vec![new.clone()],
//             connective_used: new.p.as_ref().is_some_and(|body| body.connective_used),
//             ..Par::default()
//         },
//     ])
// }
// ```

fn corrected_new_space(annotations: Vec<Option<Par>>, new: &New) -> Par {
    position_par(&ReifiedConstruct::NewSpace(NewSpace {
        annotations,
        new: new.clone(),
    }))
}

// Changed by decision 112: the annotations are the `space_types` of the `new`, one `Option` per
// plain binder. The text before:
// ```
// /// E12 (plan_nil_annotation_not_injective): the superseded "s_i or Nil" encoding maps the
// /// annotation Nil and the absence of an annotation to one term, and the substitution of Nil
// /// for an annotation variable erases the annotation. The `[]` or `[s]` encoding keeps both
// /// distinctions.
// ```
/// E12, restated by decision 112: an annotated `new` records one optional annotation per plain
/// binder, so the annotation Nil and the absence of an annotation are different terms, and the
/// substitution of Nil for an annotation variable keeps the annotation.
#[test]
// Changed by decision 112: the field form has no plan encoding to refute. The text before:
// fn reified_plan_nil_encoding_counterexample() {
fn absent_annotation_is_not_nil() {
    let space = private_name(vec![8; 32]);
    let new = new_term(2, Vec::new(), Par::default());
    let nil_annotated = [Some(Par::default()), Some(space.clone())];
    let unannotated = [None, Some(space.clone())];

    // Retired by decision 112: the plan form of the annotations has no term. The text before:
    // ```
    // assert_eq!(
    //     plan_new_space(&nil_annotated, &new),
    //     plan_new_space(&unannotated, &new)
    // );
    // ```
    assert_ne!(nil_annotated, unannotated);
    assert_ne!(
        corrected_new_space(nil_annotated.to_vec(), &new),
        corrected_new_space(unannotated.to_vec(), &new)
    );

    let env = Env::new().put(Par::default());
    let variable = [Some(bound_var(0)), Some(space.clone())];
    for variant in [Substitution::Sorted, Substitution::Unsorted] {
        // Retired by decision 112: the plan form of the annotations has no term. The text before:
        // ```
        // // The plan: a receive that gets Nil for s erases the annotation of `x : s`.
        // assert_eq!(
        //     substitute(&plan_new_space(&variable, &new), &env, variant),
        //     plan_new_space(&unannotated, &new)
        // );
        // ```
        // Changed by decision 112: the annotation is a field. The text before:
        // // The correction: the annotation becomes [Nil], which the decoder reads as Nil.
        // A receive that gets Nil for s keeps the annotation of `x : s` as `Some(Nil)`.
        let substituted = substitute(&corrected_new_space(variable.to_vec(), &new), &env, variant);
        assert_eq!(
            substituted,
            corrected_new_space(nil_annotated.to_vec(), &new)
        );
        // Changed by decision 112: the decoder reads the annotated `new`. The text before:
        // ```
        // match decode_process_tuple(&tuple_of(&substituted)) {
        //     Ok(models::rust::reified::encoding::ProcessConstruct::NewSpace(decoded)) => {
        //         assert_eq!(decoded.annotations, nil_annotated.to_vec());
        //     }
        //     other => panic!("the substituted NEWSPACE tuple decodes, got {other:?}"),
        // }
        // ```
        let decoded =
            decode_new_space(&substituted.news[0]).expect("the substituted annotated new decodes");
        assert_eq!(decoded.annotations, nil_annotated.to_vec());
    }
    // Retired by decision 112: no decoder reads a NEWSPACE tuple. The text before:
    // ```
    // // The decoder rejects the plan form outright.
    // assert_eq!(
    //     decode_process_tuple(&tuple_of(&plan_new_space(&nil_annotated, &new))),
    //     Err(ReifiedDecodeError::AnnotationShape { binder: 0 })
    // );
    // ```
}

// ── Hook errors ────────────────────────────────────────────────────────────

/// Each hook fails closed with `InterpreterError::SpaceUserError("reified:<Code>: …")` and
/// changes no state. Every code of a hook is a user failure (the user decision of 2026-10-25,
/// as DR-128), so the hooks no longer fail with `InterpreterError::ReduceError`.
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
        // Changed by decision 112: each case holds the field that its hook reads. The text before:
        // ```
        // (
        //     marker_tuple(Marker::Use, vec![gint(1)]),
        //     "reified:Malformed: the USE tuple has 2 elements instead of 3",
        // ),
        // (
        //     send(channel.clone(), vec![gint(1), sample_params()]),
        //     "reified:ParamsUnsupported: a send to this channel takes no params",
        // ),
        // (
        //     persistent_send(channel.clone(), vec![sample_params()]),
        //     "reified:ParamsUnsupported: a send to this channel takes no params",
        // ),
        // (
        //     send(channel.clone(), vec![marker_tuple(Marker::Params, vec![
        //         list(vec![]),
        //         map(vec![]),
        //     ])]),
        //     "reified:Malformed: the PARAMS tuple holds an erased empty form",
        // ),
        // (
        //     receive(sample_query(), vec![wildcard()], 0, Par::default(), false),
        //     "reified:QuerySpecUnsupported: a receive from this channel takes no query",
        // ),
        // (
        //     receive(
        //         marker_tuple(Marker::Query, vec![gint(1), list(vec![gint(2)])]),
        //         vec![wildcard()],
        //         0,
        //         Par::default(),
        //         false,
        //     ),
        //     "reified:Malformed: the QUERY tuple has 3 elements instead of 4",
        // ),
        // ```
        (
            malformed_use_block(),
            "reified:Malformed: the USE tuple is not the canonical encoding of its construct",
        ),
        (
            with_params(send(channel.clone(), vec![gint(1)]), sample_params()),
            "reified:ParamsUnsupported: a send to this channel takes no params",
        ),
        (
            with_params(
                persistent_send(channel.clone(), Vec::new()),
                sample_params(),
            ),
            "reified:ParamsUnsupported: a send to this channel takes no params",
        ),
        (
            with_params(send(channel.clone(), Vec::new()), SendParams::default()),
            "reified:Malformed: the PARAMS tuple holds an erased empty form",
        ),
        (
            with_query(
                receive(channel.clone(), vec![wildcard()], 0, Par::default(), false),
                0,
                sample_query(),
            ),
            "reified:QuerySpecUnsupported: a receive from this channel takes no query",
        ),
        (
            with_query(
                receive(channel.clone(), vec![wildcard()], 0, Par::default(), false),
                0,
                QuerySpec::default(),
            ),
            "reified:Malformed: the QUERY tuple holds an erased empty form",
        ),
    ];
    for (program, expected) in cases {
        let harness = harness(HashMap::new()).await;
        match harness.reducer.inj(program, rand()).await {
            // Changed by Reified RSpaces (user decision of 2026-10-25, as DR-128).
            // Err(InterpreterError::ReduceError(message)) => assert_eq!(message, expected),
            Err(InterpreterError::SpaceUserError(message)) => assert_eq!(message, expected),
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
            // Changed by decision 112: the builders return the field forms. The text before:
            // ```
            // &(process_constructs, arb_par(), 0i32..3),
            // |(construct, par, depth)| {
            //     let tuple = tuple_of(&position_par(&construct));
            //     let pushed = push_process_expr(par.clone(), tuple.clone());
            //     let prepended = prepend_expr(par, tuple_expr(tuple.clone()), depth);
            //     prop_assert_eq!(pushed.encode_to_vec(), prepended.encode_to_vec());
            //     if let ReifiedConstruct::NewSpace(new_space) = &construct {
            //         let expected = prepend_new(Par::default(), new_space.new.clone());
            //         prop_assert_eq!(tuple.ps[2].encode_to_vec(), expected.encode_to_vec());
            //     }
            // ```
            &(process_constructs, arb_par()),
            |(construct, par)| {
                match &construct {
                    ReifiedConstruct::Use(block) => {
                        let pushed = push_use_block(
                            par.clone(),
                            use_block(block.space.clone(), block.body.clone()),
                        );
                        let expected_free = union(
                            union(par.locally_free.clone(), block.space.locally_free.clone()),
                            block.body.locally_free.clone(),
                        );
                        prop_assert_eq!(&pushed.locally_free, &expected_free);
                        prop_assert_eq!(
                            pushed.connective_used,
                            par.connective_used
                                || block.space.connective_used
                                || block.body.connective_used
                        );
                        prop_assert_eq!(pushed.use_blocks.len(), par.use_blocks.len() + 1);
                    }
                    ReifiedConstruct::NewSpace(annotated) => {
                        let position = position_par(&construct);
                        let built = position.news[0].clone();
                        let expected_free = annotated.annotations.iter().flatten().fold(
                            annotated.new.locally_free.clone(),
                            |free, space| union(free, space.locally_free.clone()),
                        );
                        prop_assert_eq!(&built.locally_free, &expected_free);
                        let prepended = prepend_new(Par::default(), built);
                        prop_assert_eq!(&position.locally_free, &prepended.locally_free);
                        let annotation_connective = annotated
                            .annotations
                            .iter()
                            .flatten()
                            .any(|space| space.connective_used);
                        prop_assert_eq!(
                            position.connective_used,
                            prepended.connective_used || annotation_connective
                        );
                    }
                    other => prop_assert!(false, "{:?} is no process construct", other),
                }
                Ok(())
            },
        )
        // Changed by decision 112: the builders return the field forms. The text before:
        // .expect("push_process_expr and the New element follow the rholang helpers");
        .expect("the use block and the annotated new follow the rholang helpers");

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
    // Changed by decision 112: the builder returns the block, and `push_use_block` adds the flags.
    // The text before:
    // ```
    // let built = use_block(
    //     normalized_tuple.ps[0].clone(),
    //     normalized_tuple.ps[1].clone(),
    // );
    // ```
    let built = push_use_block(
        Par::default(),
        use_block(
            normalized_tuple.ps[0].clone(),
            normalized_tuple.ps[1].clone(),
        ),
    );
    assert_eq!(built.locally_free, normalized_tuple.locally_free);
    assert_eq!(built.connective_used, normalized_tuple.connective_used);
    assert!(built.connective_used, "Int is a connective");
    assert_eq!(built.locally_free, vec![1]);

    let elements = match &data[1].exprs[0].expr_instance {
        Some(ExprInstance::EListBody(elist)) => elist.ps.clone(),
        other => panic!("the second argument is a list, found {other:?}"),
    };
    // Changed by decision 112: the params keep the normalized elements in their field.
    // The text before:
    // ```
    // let params = send_params(elements.into_iter().map(SendParam::Positional).collect())
    //     .expect("positional params build")
    //     .expect("params are not empty");
    // let built_list = &tuple_of(&params).ps[1];
    // assert_eq!(built_list.encode_to_vec(), data[1].encode_to_vec());
    // ```
    let params = send_params(
        elements
            .iter()
            .cloned()
            .map(SendParam::Positional)
            .collect(),
    )
    .expect("positional params build")
    .expect("params are not empty");
    assert_eq!(params.positional, elements);

    let value = match &data[2].exprs[0].expr_instance {
        Some(ExprInstance::EMapBody(emap)) => emap.kvs[0]
            .value
            .clone()
            .expect("the map entry has a value"),
        other => panic!("the third argument is a map, found {other:?}"),
    };
    // Changed by decision 112: the params keep the normalized value in their field.
    // The text before:
    // ```
    // let params = send_params(vec![SendParam::Named("k".to_owned(), value)])
    //     .expect("named params build")
    //     .expect("params are not empty");
    // let built_map = &tuple_of(&params).ps[2];
    // assert_eq!(built_map.encode_to_vec(), data[2].encode_to_vec());
    // ```
    let params = send_params(vec![SendParam::Named("k".to_owned(), value.clone())])
        .expect("named params build")
        .expect("params are not empty");
    assert_eq!(params.named.get("k"), Some(&value));
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
        // Changed by decision 112: the retired `marker_of` gives way to a comparison with each
        // marker name. The text before:
        // assert_eq!(marker_of(name), None, "{urn}");
        assert!(!is_marker_name(name), "{urn}");
        assert!(!contains_marker(name), "{urn}");
    }
    for byte in 0..=u8::MAX {
        // Changed by decision 112: the retired `marker_of` gives way to a comparison with each
        // marker name. The text before:
        // assert_eq!(marker_of(&byte_name(byte)), None);
        assert!(!is_marker_name(&byte_name(byte)));
    }
    let tags = default_mergeable_tags();
    assert_eq!(tags.len(), 2);
    for tag in tags.keys().chain(play.reducer.mergeable_tags.keys()) {
        // Changed by decision 112: the retired `marker_of` gives way to a comparison with each
        // marker name. The text before:
        // assert_eq!(marker_of(tag), None);
        assert!(!is_marker_name(tag));
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

/// Added by decision 90 (R-1, R-2), restated by decision 112: the funding resolver counts a use
/// block with the schedule of the reducer, so it resolves a Par that holds a use block or an
/// annotated `new` at the top, in the body of a `new` and in a receive body. The resolver leaves
/// these fields unresolved, so the resolved Par keeps them as they were (R-2). A use block takes
/// one split index, as an evaluated expression does, and an annotated `new` takes the index of
/// a plain `new`, so the `new` of such a Par resolves to the names of the same Par with a
/// stand-in in place of the field.
#[test]
fn lexical_resolver_resolves_reified_pars() {
    let urn_map = HashMap::new();
    let ordinary = concatenate_pars(
        send(gstring("rrs-p2-lexical"), vec![gint(1)]),
        concatenate_pars(bound_var(0), tuple(vec![gint(1), gint(2)])),
    );
    // True when `par` holds the field of `marked`, unchanged.
    let holds_field = |par: &Par, marked: &Par| {
        marked
            .use_blocks
            .iter()
            .all(|block| par.use_blocks.contains(block))
            && marked.news.iter().all(|new| par.news.contains(new))
    };
    for marked in [sample_use(), sample_new_space()] {
        let top = concatenate_pars(ordinary.clone(), marked.clone());
        let resolved = resolve_lexical_names_for_funding(&top, rand(), &urn_map)
            .expect("a Par with a field at the top resolves");
        assert!(
            holds_field(&resolved, &marked),
            "the top field is unchanged"
        );
        let nested = new_par(1, marked.clone());
        let resolved = resolve_lexical_names_for_funding(&nested, rand(), &urn_map)
            .expect("a field in the body of a new resolves");
        assert!(
            std::iter::once(&resolved)
                .chain(resolved.news.iter().flat_map(|new| new.p.iter()))
                .any(|body| holds_field(body, &marked)),
            "the nested field is unchanged"
        );
        let received = receive(
            gstring("rrs-p2-lexical"),
            vec![wildcard()],
            0,
            marked.clone(),
            false,
        );
        assert!(resolve_lexical_names_for_funding(&received, rand(), &urn_map).is_ok());
        // A use block takes the split index of an evaluated expression, and an annotated `new`
        // takes the index of a plain `new`. The stand-in of each field takes the same index.
        let stand_in = match marked.use_blocks.as_slice() {
            [] => Par {
                news: vec![new_term(1, Vec::new(), Par::default())],
                ..Par::default()
            },
            [..] => bound_var(0),
        };
        let with_field = concatenate_pars(new_par(1, ordinary.clone()), marked);
        let with_stand_in = concatenate_pars(new_par(1, ordinary.clone()), stand_in);
        // The `new` of the ordinary term: it has no annotation, and its body holds the send.
        let names = |par: &Par| -> Vec<New> {
            resolve_lexical_names_for_funding(par, rand(), &urn_map)
                .expect("the Par resolves")
                .news
                .into_iter()
                .filter(|new| {
                    new.space_types.is_empty()
                        && new.p.as_ref().is_some_and(|body| !body.sends.is_empty())
                })
                .collect()
        };
        assert_eq!(
            names(&with_field),
            names(&with_stand_in),
            "a field takes the split index of its stand-in"
        );
    }
}

// Disabled by decision 90 (R-1): the schedule of `visit` and the test oracle count a marker tuple
// with the predicate of the reducer now, so the resolver resolves the Pars below instead of
// failing closed (`lexical_resolver_resolves_reified_pars`).
// /// The funding resolver keeps the HEAD filter, so it fails closed on a Par with a USE or
// /// NEWSPACE tuple in a resolving position (finding H-1): at the top and in the body of a `new`.
// /// A marker-free Par and an inert marker tuple resolve. The resolver does not resolve a receive
// /// body (DR-121 of `feature/cost-accounted-rho`), so a construct in a receive body resolves.
// #[test]
// fn lexical_resolver_fails_closed_on_reified_pars() {
//     let urn_map = HashMap::new();
//     let ordinary = concatenate_pars(
//         send(gstring("rrs-p2-lexical"), vec![gint(1)]),
//         concatenate_pars(bound_var(0), tuple(vec![gint(1), gint(2)])),
//     );
//     assert!(resolve_lexical_names_for_funding(&ordinary, rand(), &urn_map).is_ok());
//     let inert = concatenate_pars(ordinary.clone(), sample_params());
//     assert!(resolve_lexical_names_for_funding(&inert, rand(), &urn_map).is_ok());
//     let disagreement = Err(InterpreterError::BugFoundError(
//         LEXICAL_DISAGREEMENT.to_owned(),
//     ));
//     for marked in [sample_use(), sample_new_space()] {
//         let top = concatenate_pars(ordinary.clone(), marked.clone());
//         assert_eq!(
//             resolve_lexical_names_for_funding(&top, rand(), &urn_map),
//             disagreement
//         );
//         // Changed by decision 89 (DR-121): the resolver leaves a receive body unresolved, so
//         // the nested case that fails closed is the body of a `new`, a resolving position.
//         // let nested = receive(
//         //     gstring("rrs-p2-lexical"),
//         //     vec![wildcard()],
//         //     0,
//         //     marked,
//         //     false,
//         // );
//         // assert_eq!(
//         //     resolve_lexical_names_for_funding(&nested, rand(), &urn_map),
//         //     disagreement
//         // );
//         let nested = new_par(1, marked.clone());
//         assert_eq!(
//             resolve_lexical_names_for_funding(&nested, rand(), &urn_map),
//             disagreement
//         );
//         let received = receive(
//             gstring("rrs-p2-lexical"),
//             vec![wildcard()],
//             0,
//             marked,
//             false,
//         );
//         assert!(resolve_lexical_names_for_funding(&received, rand(), &urn_map).is_ok());
//     }
// }

/// Added by decision 90 (E-1), restated by decision 112: the cost estimate does not model the
/// demand of a use block or an annotated `new`, which the reducer runs through a space agent. So
/// a term with such a field in an execution position, at the top or in the body of a `new`, is
/// incomplete with `SpaceConstruct`, and the gate holds the signed limit. The same term without
/// the field is complete (the negative control), so the reason comes from the field alone.
#[test]
fn estimate_treats_reified_constructs_as_unmodeled_demand() {
    use models::rust::phlo_schedule::PhloScheduleV1;
    use rholang::rust::interpreter::accounting::delta_sigma::estimate::{
        linear_cost_estimate, Completeness, IncompleteReason,
    };
    use rholang::rust::interpreter::accounting::native_phlo_rules::{
        native_resource_compatibility_rule, NativePhloDimension,
    };
    use rholang::rust::interpreter::accounting::Sig;

    let schedule = PhloScheduleV1 {
        protocol_version: 6,
        network: b"test",
        shard: b"root",
        settlement_asset: b"REV",
        settlement_unit: b"atomic-REV",
        decimal_scale: 8,
        classes: NativePhloDimension::ALL
            .into_iter()
            .zip([
                b"compute".as_slice(),
                b"introduction",
                b"transfer",
                b"trace",
            ])
            .map(|(dimension, identity)| dimension.resource_class(identity, 1))
            .collect(),
        actual_price: 2,
        compatibility_rule: native_resource_compatibility_rule(),
    };
    let funding = Sig::Ground(vec![9, 9, 9, 9]);
    let urn_map = HashMap::new();
    let program = Compiler::source_to_adt("new x in { x!(0) | for (@n <- x) { Nil } }")
        .expect("the program compiles");
    let base = resolve_lexical_names_for_funding(&program, rand(), &urn_map)
        .expect("the program resolves");
    let reducer = runtime().block_on(harness(HashMap::new())).reducer;
    let completeness = |par: &Par| {
        linear_cost_estimate(par, &funding, &schedule, &reducer)
            .expect("the estimate runs")
            .completeness
    };
    assert_eq!(completeness(&base), Completeness::Complete);
    for marked in [sample_use(), sample_new_space()] {
        let top = concatenate_pars(base.clone(), marked.clone());
        assert_eq!(
            completeness(&top),
            Completeness::Incomplete(IncompleteReason::SpaceConstruct)
        );
        let nested = resolve_lexical_names_for_funding(&new_par(1, marked), rand(), &urn_map)
            .expect("a field in the body of a new resolves");
        assert_eq!(
            completeness(&nested),
            Completeness::Incomplete(IncompleteReason::SpaceConstruct)
        );
    }
}

/// Added by decision 92: every route of a deploy to a space operation makes the cost estimate
/// incomplete. So the gate holds the signed limit for each space transaction and its anchor.
///
/// A factory call sends on a URN name. A handle call and an operation on a space name use a
/// received name, and a stored handle arrives on a public channel. Rule X2 marks each of these
/// channels as not fresh. The USE and NEWSPACE routes give `SpaceConstruct`
/// (`estimate_treats_reified_constructs_as_unmodeled_demand`). Each control binds the same
/// channel with a plain `new`, and its estimate is complete, so the reason comes from the route.
#[test]
fn estimate_treats_space_routes_as_non_fresh_channels() {
    use models::rust::phlo_schedule::PhloScheduleV1;
    use rholang::rust::interpreter::accounting::delta_sigma::estimate::{
        linear_cost_estimate, Completeness, IncompleteReason,
    };
    use rholang::rust::interpreter::accounting::native_phlo_rules::{
        native_resource_compatibility_rule, NativePhloDimension,
    };
    use rholang::rust::interpreter::accounting::Sig;

    let schedule = PhloScheduleV1 {
        protocol_version: 6,
        network: b"test",
        shard: b"root",
        settlement_asset: b"REV",
        settlement_unit: b"atomic-REV",
        decimal_scale: 8,
        classes: NativePhloDimension::ALL
            .into_iter()
            .zip([
                b"compute".as_slice(),
                b"introduction",
                b"transfer",
                b"trace",
            ])
            .map(|(dimension, identity)| dimension.resource_class(identity, 1))
            .collect(),
        actual_price: 2,
        compatibility_rule: native_resource_compatibility_rule(),
    };
    let funding = Sig::Ground(vec![9, 9, 9, 9]);
    let reducer = runtime().block_on(async {
        let mut stores = InMemoryStoreManager::new();
        let store = stores
            .r_space_stores()
            .await
            .expect("create the in-memory RSpace stores");
        let (play, _replay, _history) = create_runtimes(store, false, &mut Vec::new()).await;
        play.reducer.clone()
    });
    assert!(
        reducer.urn_map.contains_key("rho:space:hashmap:queue"),
        "the runtime binds the space factories"
    );
    let completeness = |source: &str| {
        let program = Compiler::source_to_adt(source).expect("the program compiles");
        let resolved = resolve_lexical_names_for_funding(&program, rand(), &reducer.urn_map)
            .expect("the program resolves");
        linear_cost_estimate(&resolved, &funding, &schedule, &reducer)
            .expect("the estimate runs")
            .completeness
    };
    for (route, source, control) in [
        (
            "a factory call",
            r#"new factory(`rho:space:hashmap:queue`) in { factory!("default") }"#,
            r#"new factory in { factory!("default") }"#,
        ),
        (
            "a handle call on a received handle",
            r#"new reply in { for (space <- reply) { space!("gensym") } }"#,
            r#"new reply, space in { for (handle <- reply) { space!("gensym") } }"#,
        ),
        (
            "a send on a space name",
            r#"new reply in { for (a <- reply) { a!(1) } }"#,
            r#"new reply, a in { for (name <- reply) { a!(1) } }"#,
        ),
        (
            "a receive on a space name",
            r#"new reply in { for (a <- reply) { for (@x <- a) { Nil } } }"#,
            r#"new reply, a in { for (name <- reply) { for (@x <- a) { Nil } } }"#,
        ),
        (
            "a stored handle on a public channel",
            r#"for (space <- @"rrs-stored-handle") { Nil }"#,
            r#"new stash in { for (space <- stash) { Nil } }"#,
        ),
    ] {
        assert_eq!(
            completeness(control),
            Completeness::Complete,
            "{route}: the control"
        );
        assert_eq!(
            completeness(source),
            Completeness::Incomplete(IncompleteReason::NonFreshChannel),
            "{route}"
        );
    }
}

/// Added by decision 112 (decision 120 O-3, ruling 13): the classes of the estimate stay. A send
/// param takes the rule of a datum, so a param that holds a variable is `OpenData`, as a datum
/// that holds a variable is. A bind with a query keeps the class of a non-fresh channel, also
/// on a fresh channel. Each control shows that the reason comes from the param or the query.
#[test]
fn estimate_keeps_the_classes_of_params_and_queries() {
    use models::rust::phlo_schedule::PhloScheduleV1;
    use rholang::rust::interpreter::accounting::delta_sigma::estimate::{
        linear_cost_estimate, Completeness, IncompleteReason,
    };
    use rholang::rust::interpreter::accounting::native_phlo_rules::{
        native_resource_compatibility_rule, NativePhloDimension,
    };
    use rholang::rust::interpreter::accounting::Sig;

    let schedule = PhloScheduleV1 {
        protocol_version: 6,
        network: b"test",
        shard: b"root",
        settlement_asset: b"REV",
        settlement_unit: b"atomic-REV",
        decimal_scale: 8,
        classes: NativePhloDimension::ALL
            .into_iter()
            .zip([
                b"compute".as_slice(),
                b"introduction",
                b"transfer",
                b"trace",
            ])
            .map(|(dimension, identity)| dimension.resource_class(identity, 1))
            .collect(),
        actual_price: 2,
        compatibility_rule: native_resource_compatibility_rule(),
    };
    let funding = Sig::Ground(vec![9, 9, 9, 9]);
    let urn_map = HashMap::new();
    let reducer = runtime().block_on(harness(HashMap::new())).reducer;
    let completeness = |source: &str| {
        let program = Compiler::source_to_adt(source).expect("the program compiles");
        let resolved = resolve_lexical_names_for_funding(&program, rand(), &urn_map)
            .expect("the program resolves");
        linear_cost_estimate(&resolved, &funding, &schedule, &reducer)
            .expect("the estimate runs")
            .completeness
    };
    for (case, source, expected) in [
        (
            "a closed send param, the control of the data rule",
            r#"new x in { x!(0; priority=1) }"#,
            Completeness::Complete,
        ),
        (
            "a send param that holds a variable",
            r#"new x, y in { for (@n <- y) { x!(0; priority=n) } }"#,
            Completeness::Incomplete(IncompleteReason::OpenData),
        ),
        (
            "a datum that holds a variable, the same class",
            r#"new x, y in { for (@n <- y) { x!(0, n) } }"#,
            Completeness::Incomplete(IncompleteReason::OpenData),
        ),
        (
            "a bind with a query on a fresh channel",
            r#"new x in { for (@n <- x ~ 5) { Nil } }"#,
            Completeness::Incomplete(IncompleteReason::NonFreshChannel),
        ),
        (
            "a bind without a query on a fresh channel, the control",
            r#"new x in { for (@n <- x) { Nil } }"#,
            Completeness::Complete,
        ),
    ] {
        assert_eq!(completeness(source), expected, "{case}");
    }
}

/// P8 step 4 (design v2 AG-2, TS-1, W4): the space of `sample_use` and the annotation of
/// `sample_new_space` are ordinary names, so both constructs translate without an error. The
// Changed by decision 112: an annotated `new` replaces the NEWSPACE tuple. The text before:
// ```
// /// USE body sends on the forged channel `1`, which becomes one call on the agent. The NEWSPACE
// /// tuple makes one gensym call and parks its resume term.
// ```
/// USE body sends on the forged channel `1`, which becomes one call on the agent. The annotated
/// `new` makes one gensym call and parks its resume term.
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

/// Added by decision 112: a free variable of a pattern binds a whole quoted process. A process
/// with a use block is such a value, so the bound term keeps its use block.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn free_variable_binds_a_quoted_process_with_a_use_block() {
    let channel = gstring("rrs-p2-bind-use");
    let bound = gstring("rrs-p2-bind-use-out");
    let program = concatenate_pars(
        receive(
            channel.clone(),
            vec![free_var(0)],
            1,
            send(bound.clone(), vec![bound_var(0)]),
            false,
        ),
        send(channel, vec![sample_use()]),
    );
    let outcome = evaluate(program).await;
    assert_eq!(outcome.error, None, "{outcome:?}");
    assert_eq!(outcome.comms, 1, "{outcome:?}");
    assert_eq!(data_channels(&outcome), vec![bound.encode_to_vec()]);
    let datum = sample_use().encode_to_vec();
    assert!(
        outcome.store.iter().any(|entry| entry
            .windows(datum.len())
            .any(|window| window == datum.as_slice())),
        "the stored datum holds the use block: {outcome:?}"
    );
}

/// Added by decision 112: two equal `free` values are equal, they hash equally, and each matches
/// the other as a pattern.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn efree_matches_itself() {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let theory = || {
        position_par(&ReifiedConstruct::Free(
            models::rust::reified::encoding::FreeTheory {
                name: "Nat".to_owned(),
                args: vec![gint(1)],
            },
        ))
    };
    let (left, right) = (theory(), theory());
    assert_eq!(left, right);
    let digest = |par: &Par| {
        let mut hasher = DefaultHasher::new();
        par.hash(&mut hasher);
        hasher.finish()
    };
    assert_eq!(digest(&left), digest(&right));
    let channel = gstring("rrs-p2-efree");
    let program = concatenate_pars(
        receive(
            channel.clone(),
            vec![left],
            0,
            send(gstring("rrs-p2-efree-out"), vec![gint(1)]),
            false,
        ),
        send(channel, vec![right]),
    );
    let outcome = evaluate(program).await;
    assert_eq!(outcome.error, None, "{outcome:?}");
    assert_eq!(outcome.comms, 1, "{outcome:?}");
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

// Changed by decision 112: the programs hold the fields. The text before:
// ```
// /// Item 18 at P2: every marker form fails closed before it creates a consume, so the hooks
// /// add no join. An ordinary join is the control: it parks one consume.
// ```
/// Item 18 at P2: a malformed field and a field on the default space fail closed before they
/// create a consume, so the hooks add no join. An ordinary join is the control: it parks one
/// consume.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_p2_synthesizes_no_join() {
    let left = gstring("rrs-p2-join-left");
    let query = sample_query();
    // Changed by decision 112: a query without a payload is malformed, and the query names no
    // channel. The text before:
    // let malformed_query = marker_tuple(Marker::Query, vec![gint(1), list(vec![gint(2)])]);
    let malformed_query = QuerySpec::default();
    let source = gstring("rrs-p2-query");
    let programs = [
        // Changed by P8 step 4 (design v2 AG-2, W4): these constructs name user agents, so they
        // translate instead of failing closed (checked below).
        // sample_use(),
        // sample_new_space(),
        // Changed by decision 112: a malformed use block is a field without a body.
        // The text before:
        // marker_tuple(Marker::Use, vec![gint(1)]),
        malformed_use_block(),
        // new_par(1, sample_use()),
        // Changed by decision 112: a send holds its params in the field. The text before:
        // ```
        // send(left.clone(), vec![gint(1), sample_params()]),
        // persistent_send(left.clone(), vec![sample_params()]),
        // ```
        with_params(send(left.clone(), vec![gint(1)]), sample_params()),
        with_params(persistent_send(left.clone(), Vec::new()), sample_params()),
        // Changed by decision 112: a bind holds its query in the field. The text before:
        // ```
        // receive(query.clone(), vec![wildcard()], 0, Par::default(), false),
        // receive(query.clone(), vec![wildcard()], 0, Par::default(), true),
        // ```
        with_query(
            receive(source.clone(), vec![wildcard()], 0, Par::default(), false),
            0,
            query.clone(),
        ),
        with_query(
            receive(source.clone(), vec![wildcard()], 0, Par::default(), true),
            0,
            query.clone(),
        ),
        // Changed by decision 112: the query is the field of the bind of its channel.
        // The text before:
        // ```
        // join(
        //     vec![
        //         (left.clone(), vec![wildcard()], 0),
        //         (query.clone(), vec![wildcard()], 0),
        //     ],
        //     Par::default(),
        //     false,
        //     false,
        // ),
        // ```
        with_query(
            join(
                vec![
                    (left.clone(), vec![wildcard()], 0),
                    (source.clone(), vec![wildcard()], 0),
                ],
                Par::default(),
                false,
                false,
            ),
            1,
            query.clone(),
        ),
        // Changed by decision 112: the query is the field of the bind of its channel.
        // The text before:
        // ```
        // join(
        //     vec![
        //         (query.clone(), vec![wildcard()], 0),
        //         (left.clone(), vec![wildcard()], 0),
        //     ],
        //     Par::default(),
        //     false,
        //     true,
        // ),
        // ```
        with_query(
            join(
                vec![
                    (source.clone(), vec![wildcard()], 0),
                    (left.clone(), vec![wildcard()], 0),
                ],
                Par::default(),
                false,
                true,
            ),
            0,
            query.clone(),
        ),
        // Changed by decision 112: a bind holds its query in the field. The text before:
        // receive(malformed_query, vec![wildcard()], 0, Par::default(), false),
        with_query(
            receive(source.clone(), vec![wildcard()], 0, Par::default(), false),
            0,
            malformed_query,
        ),
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
        // Changed by decision 112: a field form is a process, and the tuples stay as ordinary
        // channels below. The text before:
        // ```
        // sample_params(),
        // sample_query(),
        // ```
        position_par(&ReifiedConstruct::Params(sample_params())),
        position_par(&ReifiedConstruct::Query(sample_query())),
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
        // Changed by decision 112: a field form is an ordinary channel, so no channel needs a
        // guard. The text before:
        // if index % 2 == 0 && channel != &sample_query() {
        if index % 2 == 0 {
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
