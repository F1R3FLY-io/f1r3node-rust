//! G1-4a (DR-122): parity and stack-safety tests of the estimate's walks.
//! The work-stack machines of pass 1 and pass 2 are compared with the
//! recursive oracle on 256 generated programs and on the Rholang corpus, the
//! binder kinds of pass 1 are compared with the funding resolver's levels,
//! and a term nested 100,000 deep runs through `linear_cost_estimate` on a
//! small stack (DR-67).

use std::collections::{BTreeMap, HashMap};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::cost_signature::Value as CostSignatureValue;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::{VarInstance, WildcardMsg};
use models::rhoapi::{
    Connective, CostSignature, CostSignatureCompound, CostSignedTerm, CostStack, EList, EMap, EVar,
    Expr, KeyValuePair, New, Par, Receive, ReceiveBind, Send, Var, VarRef,
};
use proptest::prelude::*;

use super::recursive_oracle::{analyze, analyze_iteratively, estimator_levels, peak_binders};
use super::tests::{evaluator, funding, runtime_urn_map, schedule, stdout_urn_map};
use super::*;
use crate::rust::interpreter::accounting::lexical::{
    resolve_lexical_names_for_funding, resolver_levels,
};
use crate::rust::interpreter::compiler::compiler::Compiler;

const DEEP_NESTING: usize = 100_000;
const ORACLE_NESTING: usize = 5_000;
const SMALL_STACK_BYTES: usize = 256 * 1024;
const ORACLE_STACK_BYTES: usize = 1024 * 1024 * 1024;

// ── A generator of Rholang programs ────────────────────────────────────────

/// One datum of a generated send. `Received` names the innermost received
/// value, which breaks rule X4.
#[derive(Clone, Debug)]
pub(super) enum GeneratedDatum {
    Int(i64),
    Text(String),
    List(Vec<GeneratedDatum>),
    Concat(String, String),
    Interpolation(u8, String),
    Received,
}

/// One process of a generated program. The complete profile uses only sends,
/// receives, joins, signed scopes, signed binds, `new` and branches on a
/// received value.
#[derive(Clone, Debug)]
pub(super) enum Generated {
    Send(u8, GeneratedDatum),
    Receive {
        binds: Vec<(u8, Option<u8>)>,
        body: Vec<Generated>,
        persistent: bool,
        peek: bool,
        /// Added by the second G1-4a fix (re-review LOW 2): the `where` guard
        /// of the receipt.
        guard: Option<GuardShape>,
    },
    Signed(u8, Vec<Generated>),
    Branch(Vec<Generated>, Vec<Generated>),
    Match(Vec<Generated>, Vec<Generated>),
    New(Vec<Generated>),
    Public(GeneratedDatum),
    Method,
    Urn,
    SignedByNew(Vec<Generated>),
    /// Added by the G1-4a fix (review H-1): a signed scope whose signature
    /// is the quote `#P`.
    SignedQuote(QuoteSignature, Vec<Generated>),
    /// Added by the second G1-4a fix (re-review LOW 2): a bundle in process
    /// position.
    Bundle(Vec<Generated>),
}

/// Added by the second G1-4a fix (re-review LOW 2): the guard of a generated
/// receive. It compares the receive's first variable with a literal, with
/// the innermost variable of an enclosing receive, or with the URN that the
/// program binds at its top.
#[derive(Clone, Debug)]
pub(super) enum GuardShape {
    Own(i64),
    Enclosing,
    Urn,
}

fn arb_guard() -> impl Strategy<Value = Option<GuardShape>> {
    prop_oneof![
        6 => Just(None),
        2 => (0i64..5).prop_map(|value| Some(GuardShape::Own(value))),
        1 => Just(Some(GuardShape::Enclosing)),
        1 => Just(Some(GuardShape::Urn)),
    ]
}

/// Added by the G1-4a fix (review H-1): the process of a generated `#P`
/// signature. A literal and the dereference of a name that the resolver
/// binds are static. The innermost received value and the dereference of a
/// fresh hole still hold a variable, so their lanes are dynamic.
#[derive(Clone, Debug)]
pub(super) enum QuoteSignature {
    Literal(i64),
    Received,
    Channel(u8),
}

fn arb_quote_signature() -> impl Strategy<Value = QuoteSignature> {
    prop_oneof![
        2 => (0i64..50).prop_map(QuoteSignature::Literal),
        1 => Just(QuoteSignature::Received),
        2 => (0u8..4).prop_map(QuoteSignature::Channel),
    ]
}

fn arb_datum(complete: bool) -> impl Strategy<Value = GeneratedDatum> {
    let leaf = prop_oneof![
        3 => (-5i64..50).prop_map(GeneratedDatum::Int),
        2 => "[a-z]{0,6}".prop_map(GeneratedDatum::Text),
        1 => ("[a-z]{0,4}", "[a-z]{0,4}").prop_map(|(left, right)| GeneratedDatum::Concat(left, right)),
        1 => (1u8..4, "[a-z]{0,8}").prop_map(|(repeats, value)| GeneratedDatum::Interpolation(repeats, value)),
        if complete { 0 } else { 1 } => Just(GeneratedDatum::Received),
    ];
    leaf.prop_recursive(2, 6, 3, |inner| {
        prop::collection::vec(inner, 0..3).prop_map(GeneratedDatum::List)
    })
}

fn arb_item(complete: bool) -> impl Strategy<Value = Generated> {
    let incomplete = if complete { 0 } else { 1 };
    let leaf = prop_oneof![
        4 => (0u8..3, arb_datum(complete)).prop_map(|(channel, datum)| Generated::Send(channel, datum)),
        incomplete => arb_datum(complete).prop_map(Generated::Public),
        incomplete => Just(Generated::Method),
        incomplete => Just(Generated::Urn),
    ];
    leaf.prop_recursive(4, 40, 4, move |inner| {
        let body = prop::collection::vec(inner, 0..3);
        prop_oneof![
            3 => (
                prop::collection::vec((0u8..3, prop::option::weighted(0.3, 0u8..2)), 1..3),
                body.clone(),
                prop::bool::weighted(if complete { 0.0 } else { 0.1 }),
                prop::bool::weighted(if complete { 0.0 } else { 0.1 }),
                arb_guard(),
            )
                .prop_map(|(binds, body, persistent, peek, guard)| Generated::Receive {
                    binds,
                    body,
                    persistent,
                    peek,
                    guard,
                }),
            2 => (0u8..3, body.clone()).prop_map(|(signature, body)| Generated::Signed(signature, body)),
            1 => (body.clone(), body.clone()).prop_map(|(left, right)| Generated::Branch(left, right)),
            1 => (body.clone(), body.clone()).prop_map(|(left, right)| Generated::Match(left, right)),
            1 => body.clone().prop_map(Generated::New),
            1 => (arb_quote_signature(), body.clone())
                .prop_map(|(signature, body)| Generated::SignedQuote(signature, body)),
            1 => body.clone().prop_map(Generated::Bundle),
            incomplete => body.prop_map(Generated::SignedByNew),
        ]
    })
}

pub(super) fn arb_program(complete: bool) -> impl Strategy<Value = Vec<Generated>> {
    prop::collection::vec(arb_item(complete), 1..4)
}

/// Renders a program as Rholang. Channels are the names of the enclosing
/// `new` terms, signatures are unbound names, and a branch tests the
/// innermost received value, or `true` outside every receive. Changed by the
/// second G1-4a fix (re-review LOW 2): the top `new` also binds the URN
/// `out`, which a generated guard can name.
pub(super) fn render(program: &[Generated]) -> String {
    // let mut out = String::from("new x0, x1, x2 in { ");
    let mut out = String::from("new x0, x1, x2, out(`rho:io:stdout`) in { ");
    let mut names = vec!["x0".to_string(), "x1".to_string(), "x2".to_string()];
    let mut received = Vec::new();
    let mut counter = 0usize;
    render_list(program, &mut names, &mut received, &mut counter, &mut out);
    out.push_str(" }");
    out
}

fn render_list(
    items: &[Generated],
    names: &mut Vec<String>,
    received: &mut Vec<String>,
    counter: &mut usize,
    out: &mut String,
) {
    if items.is_empty() {
        out.push_str("Nil");
        return;
    }
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            out.push_str(" | ");
        }
        render_item(item, names, received, counter, out);
    }
}

fn render_datum(datum: &GeneratedDatum, received: &[String], out: &mut String) {
    match datum {
        GeneratedDatum::Int(value) => out.push_str(&value.to_string()),
        GeneratedDatum::Text(text) => out.push_str(&format!("\"{text}\"")),
        GeneratedDatum::List(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                render_datum(item, received, out);
            }
            out.push(']');
        }
        GeneratedDatum::Concat(left, right) => out.push_str(&format!("\"{left}\" ++ \"{right}\"")),
        GeneratedDatum::Interpolation(repeats, value) => out.push_str(&format!(
            "\"{}\" %% {{\"a\": \"{value}\"}}",
            "${a}".repeat(usize::from(*repeats))
        )),
        GeneratedDatum::Received => match received.last() {
            Some(name) => out.push_str(name),
            None => out.push('0'),
        },
    }
}

fn render_item(
    item: &Generated,
    names: &mut Vec<String>,
    received: &mut Vec<String>,
    counter: &mut usize,
    out: &mut String,
) {
    let channel = |names: &[String], index: u8| names[usize::from(index) % names.len()].clone();
    match item {
        Generated::Send(target, datum) => {
            out.push_str(&channel(names, *target));
            out.push_str("!(");
            render_datum(datum, received, out);
            out.push(')');
        }
        Generated::Receive {
            binds,
            body,
            persistent,
            peek,
            guard,
        } => {
            let arrow = match (persistent, peek) {
                (true, _) => "<=",
                (false, true) => "<<-",
                (false, false) => "<-",
            };
            out.push_str("for (");
            let mut bound = Vec::with_capacity(binds.len());
            let mut sources: Vec<String> = Vec::with_capacity(binds.len());
            for (index, (source, signature)) in binds.iter().enumerate() {
                if index > 0 {
                    out.push_str(" & ");
                }
                *counter += 1;
                let variable = format!("v{counter}");
                let mut offset = usize::from(*source);
                let mut source = names[offset % names.len()].clone();
                while sources.contains(&source) {
                    offset += 1;
                    source = names[offset % names.len()].clone();
                }
                sources.push(source.clone());
                match signature {
                    Some(signature) if !persistent && !peek => {
                        out.push_str(&format!("{{% @{variable} <- {source} %}}[ b{signature} ]"))
                    }
                    _ => out.push_str(&format!("@{variable} {arrow} {source}")),
                }
                bound.push(variable);
            }
            if let (Some(guard), Some(own)) = (guard, bound.first()) {
                let other = match guard {
                    GuardShape::Own(value) => value.to_string(),
                    GuardShape::Enclosing => match received.last() {
                        Some(name) => name.clone(),
                        None => "0".to_string(),
                    },
                    GuardShape::Urn => "*out".to_string(),
                };
                let operator = match guard {
                    GuardShape::Own(_) => ">",
                    GuardShape::Enclosing | GuardShape::Urn => "==",
                };
                out.push_str(&format!(" where {own} {operator} {other}"));
            }
            out.push_str(") { ");
            let depth = received.len();
            received.extend(bound);
            render_list(body, names, received, counter, out);
            received.truncate(depth);
            out.push_str(" }");
        }
        Generated::Signed(signature, body) => {
            out.push_str("{% ");
            render_list(body, names, received, counter, out);
            out.push_str(&format!(" %}}[ s{signature} ]"));
        }
        Generated::Branch(left, right) => {
            let condition = match received.last() {
                Some(name) => format!("{name} == 3"),
                None => "true".to_string(),
            };
            out.push_str(&format!("if ({condition}) {{ "));
            render_list(left, names, received, counter, out);
            out.push_str(" } else { ");
            render_list(right, names, received, counter, out);
            out.push_str(" }");
        }
        Generated::Match(left, right) => {
            let target = received.last().cloned().unwrap_or_else(|| "7".to_string());
            out.push_str(&format!("match {target} {{ 1 => {{ "));
            render_list(left, names, received, counter, out);
            out.push_str(" } _ => { ");
            render_list(right, names, received, counter, out);
            out.push_str(" } }");
        }
        Generated::New(body) => {
            *counter += 1;
            let name = format!("y{counter}");
            out.push_str(&format!("new {name} in {{ "));
            names.push(name);
            render_list(body, names, received, counter, out);
            names.pop();
            out.push_str(" }");
        }
        Generated::Public(datum) => {
            out.push_str("@\"public\"!(");
            render_datum(datum, received, out);
            out.push(')');
        }
        Generated::Method => out.push_str("[ @\"public\"!(0) ].nth(0)"),
        Generated::Urn => {
            out.push_str("new out(`rho:io:stdout`) in { out!(1) }");
        }
        Generated::SignedByNew(body) => {
            *counter += 1;
            let name = format!("z{counter}");
            out.push_str(&format!("new {name} in {{ {{% "));
            render_list(body, names, received, counter, out);
            out.push_str(&format!(" %}}[ {name} ] }}"));
        }
        Generated::Bundle(body) => {
            out.push_str("bundle+ { ");
            render_list(body, names, received, counter, out);
            out.push_str(" }");
        }
        Generated::SignedQuote(signature, body) => {
            let quoted = match signature {
                QuoteSignature::Literal(value) => value.to_string(),
                QuoteSignature::Received => match received.last() {
                    Some(name) => name.clone(),
                    None => "7".to_string(),
                },
                QuoteSignature::Channel(index) => format!("*{}", channel(names, *index)),
            };
            out.push_str("{% ");
            render_list(body, names, received, counter, out);
            out.push_str(&format!(" %}}[ #{quoted} ]"));
        }
    }
}

fn compile(source: &str) -> Option<Par> {
    std::panic::catch_unwind(|| Compiler::source_to_adt(source))
        .ok()
        .and_then(Result::ok)
}

/// Runs both analyses of one program on a thread with a large stack, where
/// the oracle's recursion fits.
fn assert_parity(source: &str, urn_map: &HashMap<String, Par>) -> Result<(), TestCaseError> {
    let Some(program) = compile(source) else {
        return Ok(());
    };
    let resolved = match resolve_lexical_names_for_funding(
        &program,
        Blake2b512Random::create_from_bytes(b"estimate parity"),
        urn_map,
    ) {
        Ok(resolved) => resolved,
        Err(_) => {
            dismantle_term(program);
            return Ok(());
        }
    };
    dismantle_term(program);
    let reducer = evaluator(urn_map.clone());
    let (iterative, recursive) = std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("estimate-parity".to_string())
            .stack_size(ORACLE_STACK_BYTES)
            .spawn_scoped(scope, || {
                let schedule = schedule([2, 1, 1, 3]);
                (
                    analyze_iteratively(&resolved, &funding(), &schedule, &reducer),
                    analyze(&resolved, &funding(), &schedule, &reducer),
                )
            })
            .expect("spawn the parity thread")
            .join()
            .expect("both analyses finish")
    });
    dismantle_term(resolved);
    prop_assert_eq!(iterative, recursive, "{}", source);
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// The work-stack machines of pass 1 and pass 2 give the participants,
    /// the first failed rule, the vector and the estimate of the recursive
    /// oracle on every generated program.
    #[test]
    fn iterative_passes_match_the_recursive_oracle(program in arb_program(false)) {
        assert_parity(&render(&program), &stdout_urn_map())?;
    }
}

/// The Rholang sources of the repository, as the analyzer tests read them.
fn corpus_sources() -> Vec<(std::path::PathBuf, String)> {
    let crate_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut pending = vec![
        crate_root.join("../examples"),
        crate_root.join("examples"),
        crate_root.join("src/test/resources/tests"),
        crate_root.join("../casper/src/main/resources"),
    ];
    let mut sources = Vec::new();
    while let Some(path) = pending.pop() {
        if path.is_dir() {
            let mut entries = std::fs::read_dir(&path)
                .expect("read a corpus directory")
                .map(|entry| entry.expect("read a corpus entry").path())
                .collect::<Vec<_>>();
            entries.sort();
            pending.extend(entries);
        } else if path.extension().is_some_and(|extension| extension == "rho") {
            let source = std::fs::read_to_string(&path).expect("read a corpus source");
            sources.push((path, source));
        }
    }
    sources.sort();
    sources
}

/// On every corpus program that the normalizer accepts, the binder count of
/// pass 1 at each node that it visits equals the funding resolver's count at
/// that node, and both analyses agree.
#[test]
fn binder_levels_and_parity_hold_on_the_corpus() {
    let urn_map = runtime_urn_map();
    let reducer = evaluator(urn_map.clone());
    let mut compiled = 0usize;
    let mut checked = 0usize;
    for (path, source) in corpus_sources() {
        let Some(program) = compile(&source) else {
            continue;
        };
        compiled += 1;
        let rand = Blake2b512Random::create_from_bytes(b"corpus levels");
        let Ok(resolver) = resolver_levels(&program, rand, &urn_map) else {
            dismantle_term(program);
            continue;
        };
        checked += 1;
        let resolver: HashMap<usize, usize> = resolver.into_iter().collect();
        let levels =
            estimator_levels(&program, &funding(), &reducer).expect("pass 1 walks the program");
        assert!(!levels.is_empty(), "{}", path.display());
        for (address, level) in levels {
            assert_eq!(
                resolver.get(&address).copied(),
                Some(level),
                "binder count of {}",
                path.display()
            );
        }
        let result = assert_parity(&source, &urn_map);
        dismantle_term(program);
        result.unwrap_or_else(|error| panic!("parity of {}: {error}", path.display()));
    }
    assert!(compiled >= 40, "only {compiled} corpus sources compiled");
    assert!(
        checked >= 30,
        "only {checked} of {compiled} compiled corpus sources resolved"
    );
}

// ── A term nested 100,000 deep ─────────────────────────────────────────────

fn bound(level: i32) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EVarBody(EVar {
            v: Some(Var {
                var_instance: Some(VarInstance::BoundVar(level)),
            }),
        })),
    }])
}

/// `depth` levels, each a `new` of one name whose body sends on the name and
/// receives on it, with the next level inside the receive body. The loop
/// builds the term from the inside out, so construction never recurses.
fn deep_term(depth: usize) -> Par {
    let mut par = Par::default();
    for _ in 0..depth {
        let mut level = Par::default();
        level.sends.push(Send {
            chan: Some(bound(0)),
            data: vec![Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::GInt(1)),
            }])],
            persistent: false,
            locally_free: vec![1],
            connective_used: false,
        });
        level.receives.push(Receive {
            binds: vec![ReceiveBind {
                patterns: Vec::new(),
                source: Some(bound(0)),
                remainder: None,
                free_count: 0,
                cost_signature: None,
            }],
            body: Some(par),
            persistent: false,
            peek: false,
            bind_count: 0,
            locally_free: vec![1],
            connective_used: false,
            condition: None,
        });
        let mut outer = Par::default();
        outer.news.push(New {
            bind_count: 1,
            p: Some(level),
            uri: Vec::new(),
            injections: BTreeMap::new(),
            locally_free: Vec::new(),
        });
        par = outer;
    }
    par
}

/// The estimate of a term nested 100,000 levels deep finishes on a 256 KiB
/// stack, through the passes and through the public entry. The receive bodies
/// above the sizing bound have no size, so the estimate is incomplete.
#[test]
fn the_estimate_runs_a_term_nested_one_hundred_thousand_deep_on_a_small_stack() {
    let par = deep_term(DEEP_NESTING);
    let reducer = evaluator(HashMap::new());
    let (iterative, direct) = std::thread::scope(|scope| {
        let iterative = std::thread::Builder::new()
            .name("estimate-small-stack".to_string())
            .stack_size(SMALL_STACK_BYTES)
            .spawn_scoped(scope, || {
                analyze_iteratively(&par, &funding(), &schedule([1; 4]), &reducer)
            })
            .expect("spawn the small-stack thread")
            .join();
        let direct = std::thread::Builder::new()
            .name("estimate-entry".to_string())
            .stack_size(SMALL_STACK_BYTES)
            .spawn_scoped(scope, || {
                linear_cost_estimate(&par, &funding(), &schedule([1; 4]), &reducer)
            })
            .expect("spawn the entry thread")
            .join();
        (iterative, direct)
    });
    std::thread::Builder::new()
        .name("estimate-deep-teardown".to_string())
        .stack_size(SMALL_STACK_BYTES)
        .spawn(move || dismantle_term(par))
        .expect("spawn the teardown thread")
        .join()
        .expect("the iterative teardown stays within a small stack");
    let iterative = iterative
        .expect("the iterative passes stay within a small stack")
        .expect("the estimate runs");
    let direct = direct
        .expect("the public entry stays within a small stack")
        .expect("the public entry runs");
    assert_eq!(
        iterative.estimate.completeness,
        Completeness::Incomplete(IncompleteReason::UnsizedValue)
    );
    assert_eq!(
        iterative.participants.len(),
        2 * DEEP_NESTING,
        "every level holds one send and one receive"
    );
    assert_eq!(direct, iterative.estimate);
}

/// The recursive oracle agrees with the work-stack passes on a term deep
/// enough that its outer receive bodies exceed the sizing bound. The oracle
/// recurses once per level, so this comparison uses a depth whose frames fit
/// the oracle's thread in a debug build.
#[test]
fn the_oracle_agrees_on_a_term_above_the_sizing_bound() {
    let par = deep_term(ORACLE_NESTING);
    let reducer = evaluator(HashMap::new());
    let (iterative, recursive) = std::thread::scope(|scope| {
        let iterative = std::thread::Builder::new()
            .name("estimate-small-stack".to_string())
            .stack_size(SMALL_STACK_BYTES)
            .spawn_scoped(scope, || {
                analyze_iteratively(&par, &funding(), &schedule([1; 4]), &reducer)
            })
            .expect("spawn the small-stack thread")
            .join();
        let recursive = std::thread::Builder::new()
            .name("estimate-oracle".to_string())
            .stack_size(ORACLE_STACK_BYTES)
            .spawn_scoped(scope, || {
                analyze(&par, &funding(), &schedule([1; 4]), &reducer)
            })
            .expect("spawn the oracle thread")
            .join();
        (iterative, recursive)
    });
    std::thread::Builder::new()
        .name("estimate-oracle-teardown".to_string())
        .stack_size(SMALL_STACK_BYTES)
        .spawn(move || dismantle_term(par))
        .expect("spawn the teardown thread")
        .join()
        .expect("the iterative teardown stays within a small stack");
    let iterative = iterative
        .expect("the iterative passes stay within a small stack")
        .expect("the estimate runs");
    let recursive = recursive
        .expect("the oracle runs on a large stack")
        .expect("the oracle estimate runs");
    assert_eq!(
        iterative.estimate.completeness,
        Completeness::Incomplete(IncompleteReason::UnsizedValue)
    );
    assert_eq!(iterative, recursive);
}

/// `depth` nested lists around an integer: a datum whose message height grows
/// by two with each list.
fn nested_list(depth: usize) -> Par {
    let mut par = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(7)),
    }]);
    for _ in 0..depth {
        par = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EListBody(models::rhoapi::EList {
                ps: vec![par],
                ..Default::default()
            })),
        }]);
    }
    par
}

/// A datum at the sizing bound is evaluated, substituted and measured on a
/// 256 KiB thread, because those recursive calls run on the explicit stack
/// segment (arbitration M4). A datum one list above the bound has no size.
#[test]
fn a_datum_at_the_sizing_bound_is_sized_on_a_small_stack() {
    let reducer = evaluator(HashMap::new());
    let core = isolated_evaluator(&reducer);
    let probe = Estimator::new(&funding(), &core).expect("an estimator");
    let env_height = probe.env_height;
    let fits = |height: u32| height + env_height + WRAPPER_MARGIN <= SIZING_HEIGHT_LIMIT;
    let mut depth = 0usize;
    while fits(measured_height(&nested_list(depth + 1))) {
        depth += 1;
    }
    for (lists, sized) in [(depth, true), (depth + 1, false)] {
        let send = |datum: Par| {
            let mut level = Par::default();
            level.sends.push(Send {
                chan: Some(bound(0)),
                data: vec![datum],
                persistent: false,
                locally_free: vec![1],
                connective_used: false,
            });
            let mut root = Par::default();
            root.news.push(New {
                bind_count: 1,
                p: Some(level),
                uri: Vec::new(),
                injections: BTreeMap::new(),
                locally_free: Vec::new(),
            });
            root
        };
        let par = send(nested_list(lists));
        let result = std::thread::scope(|scope| {
            std::thread::Builder::new()
                .name("estimate-bound".to_string())
                .stack_size(SMALL_STACK_BYTES)
                .spawn_scoped(scope, || {
                    analyze_iteratively(&par, &funding(), &schedule([1; 4]), &reducer)
                })
                .expect("spawn the bound thread")
                .join()
                .expect("the sizing stays within its stack segment")
                .expect("the estimate runs")
        });
        let send_size = match result.participants.first() {
            Some(Participant::Send { introduction, .. }) => *introduction,
            _ => None,
        };
        assert_eq!(send_size.is_some(), sized, "{lists} nested lists");
        dismantle_term(par);
    }
}

fn measured_height(par: &Par) -> u32 {
    let mut measures = Measures::default();
    measure_subterm(Subterm::Par(par), &mut measures)
        .expect("the value is measured")
        .height
}

/// Draws `count` programs of a profile from a deterministic runner.
pub(super) fn sample_programs(complete: bool, count: usize) -> Vec<String> {
    use proptest::strategy::ValueTree;
    use proptest::test_runner::TestRunner;

    let mut runner = TestRunner::deterministic();
    let strategy = arb_program(complete);
    (0..count)
        .map(|_| {
            render(
                &strategy
                    .new_tree(&mut runner)
                    .expect("a generated program")
                    .current(),
            )
        })
        .collect()
}

/// The parity property is not vacuous: most generated programs compile and
/// resolve, and the sample holds both complete and incomplete estimates.
#[test]
fn generated_programs_reach_both_analyses() {
    let urn_map = stdout_urn_map();
    let reducer = evaluator(urn_map.clone());
    let mut compiled = 0usize;
    let mut complete = 0usize;
    let mut incomplete = 0usize;
    for source in sample_programs(false, 64) {
        let Some(program) = compile(&source) else {
            continue;
        };
        let resolved = resolve_lexical_names_for_funding(
            &program,
            Blake2b512Random::create_from_bytes(b"estimate parity"),
            &urn_map,
        );
        dismantle_term(program);
        let Ok(resolved) = resolved else {
            continue;
        };
        compiled += 1;
        let estimate = linear_cost_estimate(&resolved, &funding(), &schedule([1; 4]), &reducer)
            .expect("the estimate runs");
        dismantle_term(resolved);
        match estimate.completeness {
            Completeness::Complete => complete += 1,
            Completeness::Incomplete(_) => incomplete += 1,
        }
    }
    assert!(
        compiled >= 56,
        "only {compiled} of 64 generated programs compiled"
    );
    assert!(complete >= 8, "only {complete} complete estimates");
    assert!(incomplete >= 8, "only {incomplete} incomplete estimates");
}

// ── The variable scan of a signature (review H-1) ──────────────────────────

fn scanned_variable(instance: VarInstance) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EVarBody(EVar {
            v: Some(Var {
                var_instance: Some(instance),
            }),
        })),
    }])
}

fn scanned_integer(value: i64) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(value)),
    }])
}

fn scanned_signature(value: CostSignatureValue) -> CostSignature {
    CostSignature { value: Some(value) }
}

/// A list of one integer, open with a remainder when `open` holds.
fn scanned_list(open: bool) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EListBody(EList {
            ps: vec![scanned_integer(1)],
            remainder: open.then_some(Var {
                var_instance: Some(VarInstance::FreeVar(0)),
            }),
            ..Default::default()
        })),
    }])
}

/// A map of one entry, open with a remainder when `open` holds.
fn scanned_map(open: bool) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EMapBody(EMap {
            kvs: vec![KeyValuePair {
                key: Some(scanned_integer(1)),
                value: Some(scanned_integer(2)),
            }],
            remainder: open.then_some(Var {
                var_instance: Some(VarInstance::FreeVar(0)),
            }),
            ..Default::default()
        })),
    }])
}

/// A receive on an integer channel whose bind holds `pattern`, a remainder
/// when `open` holds, and the bind signature `signature`.
fn scanned_receive(pattern: Par, open: bool, signature: Option<CostSignature>) -> Par {
    Par::default().with_receives(vec![Receive {
        binds: vec![ReceiveBind {
            patterns: vec![pattern],
            source: Some(scanned_integer(3)),
            remainder: open.then_some(Var {
                var_instance: Some(VarInstance::FreeVar(0)),
            }),
            free_count: 0,
            cost_signature: signature,
        }],
        body: Some(Par::default()),
        ..Default::default()
    }])
}

/// One leaf process of a scanned signature: closed, or holding one kind of
/// variable. The kinds are a bound or free variable, a wildcard, a variable
/// reference, and the remainder of a list, a map or a bind.
fn arb_scanned_leaf() -> impl Strategy<Value = Par> {
    prop_oneof![
        3 => Just(Par::default()),
        3 => (0i64..4).prop_map(scanned_integer),
        1 => (0i32..3).prop_map(|level| scanned_variable(VarInstance::BoundVar(level))),
        1 => (0i32..3).prop_map(|level| scanned_variable(VarInstance::FreeVar(level))),
        1 => Just(scanned_variable(VarInstance::Wildcard(WildcardMsg {}))),
        1 => Just(Par::default().with_connectives(vec![Connective {
            connective_instance: Some(ConnectiveInstance::VarRefBody(VarRef { index: 0, depth: 1 })),
        }])),
        2 => any::<bool>().prop_map(scanned_list),
        2 => any::<bool>().prop_map(scanned_map),
        2 => any::<bool>().prop_map(|open| scanned_receive(Par::default(), open, None)),
    ]
}

/// Every signature form, with `Quote` and `Name` atoms around processes
/// that nest further signatures: in a signed term, in a token stack, in the
/// data of a send and in the signature of a bind.
fn arb_scanned_signature() -> impl Strategy<Value = CostSignature> {
    let leaf = prop_oneof![
        2 => (0u8..4).prop_map(|tag| scanned_signature(CostSignatureValue::Ground(vec![tag]))),
        1 => Just(scanned_signature(CostSignatureValue::Unit(true))),
        1 => (0i32..3).prop_map(|level| scanned_signature(CostSignatureValue::BoundLevel(level))),
        3 => arb_scanned_leaf().prop_map(|par| scanned_signature(CostSignatureValue::Quote(par))),
        2 => arb_scanned_leaf().prop_map(|par| scanned_signature(CostSignatureValue::Name(par))),
    ];
    leaf.prop_recursive(4, 48, 3, |inner| {
        prop_oneof![
            2 => prop::collection::vec(inner.clone(), 0..3).prop_map(|elements| {
                scanned_signature(CostSignatureValue::Compound(CostSignatureCompound { elements }))
            }),
            2 => (inner.clone(), arb_scanned_leaf()).prop_map(|(signature, body)| {
                scanned_signature(CostSignatureValue::Quote(Par {
                    cost_signed_terms: vec![CostSignedTerm {
                        body: Some(body),
                        signature: Some(signature),
                    }],
                    ..Default::default()
                }))
            }),
            1 => inner.clone().prop_map(|signature| {
                scanned_signature(CostSignatureValue::Name(Par {
                    cost_stacks: vec![CostStack {
                        cells: vec![signature],
                    }],
                    ..Default::default()
                }))
            }),
            1 => (inner.clone(), arb_scanned_leaf()).prop_map(|(signature, datum)| {
                let signed = Par {
                    cost_signed_terms: vec![CostSignedTerm {
                        body: None,
                        signature: Some(signature),
                    }],
                    ..Default::default()
                };
                scanned_signature(CostSignatureValue::Quote(Par::default().with_sends(vec![Send {
                    chan: Some(scanned_integer(4)),
                    data: vec![datum, signed],
                    persistent: false,
                    locally_free: Vec::new(),
                    connective_used: false,
                }])))
            }),
            1 => (inner, arb_scanned_leaf()).prop_map(|(signature, pattern)| {
                scanned_signature(CostSignatureValue::Quote(scanned_receive(
                    pattern,
                    false,
                    Some(signature),
                )))
            }),
        ]
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Review H-1: the work-stack scan of a signature for a variable gives the
    /// answer of the recursive oracle on every generated signature.
    #[test]
    fn the_variable_scan_matches_the_recursive_oracle(signature in arb_scanned_signature()) {
        prop_assert_eq!(
            signature_holds_variable(&signature),
            super::recursive_oracle::signature_holds_variable(&signature)
        );
    }
}

/// The parity property of the variable scan is not vacuous: the generated
/// signatures hold a variable in many cases and none in many others.
#[test]
fn generated_scanned_signatures_reach_both_answers() {
    use proptest::strategy::ValueTree;
    use proptest::test_runner::TestRunner;

    let mut runner = TestRunner::deterministic();
    let strategy = arb_scanned_signature();
    let mut holding = 0usize;
    for _ in 0..256 {
        let signature = strategy
            .new_tree(&mut runner)
            .expect("a generated signature")
            .current();
        if signature_holds_variable(&signature) {
            holding += 1;
        }
    }
    assert!(
        (32..=224).contains(&holding),
        "{holding} of 256 generated signatures hold a variable"
    );
}

/// A signature nested `depth` levels deep: each level quotes a signed term
/// whose signature is the next level. The innermost signature is `bottom`.
/// The loop builds it from the inside out, so construction never recurses.
fn deep_signature(depth: usize, bottom: CostSignature) -> CostSignature {
    let mut signature = bottom;
    for _ in 0..depth {
        signature = scanned_signature(CostSignatureValue::Quote(Par {
            cost_signed_terms: vec![CostSignedTerm {
                body: None,
                signature: Some(signature),
            }],
            ..Default::default()
        }));
    }
    signature
}

/// Review H-1 and DR-67: the scan finishes on a signature nested 100,000
/// levels deep on a 256 KiB stack. It finds a variable at the bottom and
/// reports none above a ground bottom. The recursive oracle agrees at a depth
/// whose recursion fits its large stack. Each signature is dropped through
/// `dismantle_term`.
#[test]
fn the_variable_scan_runs_a_signature_nested_one_hundred_thousand_deep_on_a_small_stack() {
    let variable = || scanned_signature(CostSignatureValue::BoundLevel(0));
    let ground = || scanned_signature(CostSignatureValue::Ground(vec![1]));
    let dismantle_signature = |signature: CostSignature| {
        std::thread::Builder::new()
            .name("scan-teardown".to_string())
            .stack_size(SMALL_STACK_BYTES)
            .spawn(move || {
                dismantle_term(Par {
                    cost_signed_terms: vec![CostSignedTerm {
                        body: None,
                        signature: Some(signature),
                    }],
                    ..Default::default()
                })
            })
            .expect("spawn the teardown thread")
            .join()
            .expect("the iterative teardown stays within a small stack");
    };
    for (bottom, expected) in [(variable(), true), (ground(), false)] {
        let deep = deep_signature(DEEP_NESTING, bottom.clone());
        let oracle_input = deep_signature(ORACLE_NESTING, bottom);
        let (scanned, oracle_scanned, oracle) = std::thread::scope(|scope| {
            let scanned = std::thread::Builder::new()
                .name("scan-small-stack".to_string())
                .stack_size(SMALL_STACK_BYTES)
                .spawn_scoped(scope, || signature_holds_variable(&deep))
                .expect("spawn the scan thread")
                .join()
                .expect("the scan stays within a small stack");
            let oracle_scanned = std::thread::Builder::new()
                .name("scan-oracle-input".to_string())
                .stack_size(SMALL_STACK_BYTES)
                .spawn_scoped(scope, || signature_holds_variable(&oracle_input))
                .expect("spawn the scan thread")
                .join()
                .expect("the scan stays within a small stack");
            let oracle = std::thread::Builder::new()
                .name("scan-oracle".to_string())
                .stack_size(ORACLE_STACK_BYTES)
                .spawn_scoped(scope, || {
                    super::recursive_oracle::signature_holds_variable(&oracle_input)
                })
                .expect("spawn the oracle thread")
                .join()
                .expect("the oracle runs on a large stack");
            (scanned, oracle_scanned, oracle)
        });
        dismantle_signature(deep);
        dismantle_signature(oracle_input);
        assert_eq!(
            (scanned, oracle_scanned, oracle),
            (expected, expected, expected)
        );
    }
}

// ── The peak binder count (re-review HIGH-1) ───────────────────────────────

/// Re-review HIGH-1: the peak binder count of the pre-pass bounds the
/// binders that pass 1 holds at once, on generated programs and on the
/// corpus, so the environment map never grows. On a term whose positions
/// all belong to pass 1, the two counts are equal.
#[test]
fn the_peak_binder_count_bounds_pass_one() {
    let urn_map = runtime_urn_map();
    let reducer = evaluator(urn_map.clone());
    let mut checked = 0usize;
    let sources = sample_programs(false, 64)
        .into_iter()
        .chain(corpus_sources().into_iter().map(|(_, source)| source));
    for source in sources {
        let Some(program) = compile(&source) else {
            continue;
        };
        let resolved = resolve_lexical_names_for_funding(
            &program,
            Blake2b512Random::create_from_bytes(b"peak binders"),
            &urn_map,
        );
        dismantle_term(program);
        let Ok(resolved) = resolved else {
            continue;
        };
        let (peak, observed) =
            peak_binders(&resolved, &funding(), &reducer).expect("pass 1 walks the program");
        dismantle_term(resolved);
        assert!(observed <= peak, "{observed} above {peak}: {source}");
        checked += 1;
    }
    assert!(checked >= 80, "only {checked} programs were checked");
    let par = deep_term(1_000);
    let counts = peak_binders(&par, &funding(), &reducer).expect("pass 1 walks the deep term");
    dismantle_term(par);
    assert_eq!(counts, (1_000, 1_000));
}

// ── The compute row of the plan (re-review MEDIUM-2) ───────────────────────

/// Checks that the compute count of the plan is at least the compute count
/// of the vector for every lane where rule X1 holds. It returns whether the
/// program reached that comparison with a positive compute count.
fn assert_plan_dominates(
    source: &str,
    urn_map: &HashMap<String, Par>,
) -> Result<bool, TestCaseError> {
    let Some(program) = compile(source) else {
        return Ok(false);
    };
    let resolved = resolve_lexical_names_for_funding(
        &program,
        Blake2b512Random::create_from_bytes(b"plan dominance"),
        urn_map,
    );
    dismantle_term(program);
    let Ok(resolved) = resolved else {
        return Ok(false);
    };
    let reducer = evaluator(urn_map.clone());
    let analysis = analyze_iteratively(&resolved, &funding(), &schedule([1; 4]), &reducer);
    let plan = static_authority_plan(&resolved, &funding());
    dismantle_term(resolved);
    let analysis = analysis.map_err(|error| TestCaseError::fail(format!("{source}: {error}")))?;
    let Ok(plan) = plan else {
        return Ok(false);
    };
    for (lane, counts) in &analysis.vector {
        let planned = plan.demand.0.get(lane).copied().unwrap_or(0);
        prop_assert!(
            planned >= counts[0],
            "{}: lane {:?} plan {} below vector {}",
            source,
            &lane[..4],
            planned,
            counts[0]
        );
    }
    Ok(analysis.vector.values().any(|counts| counts[0] > 0))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Re-review MEDIUM-2: where rule X1 holds, the compute count of the plan
    /// is at least the compute count of the vector for every lane, on every
    /// generated program of both profiles.
    #[test]
    fn the_plan_compute_row_dominates_the_vector_row(program in arb_program(false)) {
        assert_plan_dominates(&render(&program), &stdout_urn_map())?;
    }
}

/// Re-review MEDIUM-2: the dominance holds on the corpus, and the property
/// is not vacuous: many generated programs reach the comparison with a
/// positive compute count.
#[test]
fn the_plan_compute_row_dominates_on_the_corpus() {
    let urn_map = stdout_urn_map();
    let reached = sample_programs(false, 64)
        .iter()
        .chain(sample_programs(true, 32).iter())
        .filter(|source| {
            assert_plan_dominates(source, &urn_map)
                .unwrap_or_else(|error| panic!("dominance: {error}"))
        })
        .count();
    assert!(
        reached >= 24,
        "only {reached} of 96 programs reached the comparison"
    );
    let runtime = runtime_urn_map();
    for (path, source) in corpus_sources() {
        assert_plan_dominates(&source, &runtime)
            .unwrap_or_else(|error| panic!("dominance on {}: {error}", path.display()));
    }
}
