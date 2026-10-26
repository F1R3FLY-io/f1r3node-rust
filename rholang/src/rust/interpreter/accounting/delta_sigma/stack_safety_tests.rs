//! G1-1 (DR-67): parity and stack-safety tests of the iterative analyzer walks.
//!
//! The walks of `delta_sigma.rs` are a verbatim backport of merge `29b729551`
//! (branch `integration/f1r3lang-cost-accounted-rho-20261005`). This file
//! compares them with the recursive walks that they replaced, which stay as
//! the `#[cfg(test)]` module `recursive_oracle`. It also runs them on a term
//! that is nested 100,000 levels deep. The tests live in their own file, so
//! that a later F1R3Lang merge of this branch does not meet them next to the
//! F1R3Lang edits of the in-file tests.

use std::collections::BTreeMap;

use models::rhoapi::cost_signature::Value as CostSignatureValue;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::{VarInstance, WildcardMsg};
use models::rhoapi::{
    Bundle, CostSignature, CostSignatureCompound, CostSignedTerm, CostStack, EVar, Expr, If, Match,
    MatchCase, New, Par, Receive, ReceiveBind, Send, Var,
};
use proptest::prelude::*;

use super::{recursive_oracle, *};

/// The nesting depth of the deep test. It is far beyond the depth at which the
/// recursive walks exhaust a small thread stack.
const DEEP_NESTING: usize = 100_000;

/// The stack of the thread that runs the iterative walks on the deep term.
const SMALL_STACK_BYTES: usize = 256 * 1024;

/// The stack of the thread that runs the recursive oracle on the deep term. The
/// operating system reserves it lazily, so only the touched frames use memory.
const ORACLE_STACK_BYTES: usize = 1024 * 1024 * 1024;

fn ground(tag: u8) -> CostSignature {
    CostSignature {
        value: Some(CostSignatureValue::Ground(vec![tag])),
    }
}

fn compound(elements: Vec<CostSignature>) -> CostSignature {
    CostSignature {
        value: Some(CostSignatureValue::Compound(CostSignatureCompound {
            elements,
        })),
    }
}

fn bound_level(level: i32) -> CostSignature {
    CostSignature {
        value: Some(CostSignatureValue::BoundLevel(level)),
    }
}

fn unit(canonical: bool) -> CostSignature {
    CostSignature {
        value: Some(CostSignatureValue::Unit(canonical)),
    }
}

/// A channel that carries the integer `tag`. [`region_sig`] maps the channels
/// with an even tag to a lane and leaves the others to the envelope lane.
fn channel(tag: i64) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(tag)),
    }])
}

fn region_sig(channel: &Par) -> Option<SigKey> {
    match channel
        .exprs
        .first()
        .and_then(|expr| expr.expr_instance.as_ref())
    {
        Some(ExprInstance::GInt(tag)) if tag % 2 == 0 => Some([(*tag as u8) ^ 0x5a; 32]),
        _ => None,
    }
}

fn dequotation(instance: Option<VarInstance>) -> Expr {
    Expr {
        expr_instance: Some(ExprInstance::EVarBody(EVar {
            v: Some(Var {
                var_instance: instance,
            }),
        })),
    }
}

/// Every cost signature shape that the walks distinguish: static ground lanes
/// that collide on purpose, the unit, a non-canonical unit, a dynamic bound
/// level, canonical and non-canonical compounds, a malformed one-element
/// compound, a compound with a dynamic element, a missing value and a missing
/// signature.
fn arb_signature() -> impl Strategy<Value = Option<CostSignature>> {
    prop_oneof![
        6 => (0u8..4).prop_map(|tag| Some(ground(tag))),
        1 => Just(Some(unit(true))),
        1 => Just(Some(unit(false))),
        2 => (0i32..3).prop_map(|level| Some(bound_level(level))),
        2 => (0u8..4, 0u8..4).prop_map(|(left, right)| Some(compound(vec![ground(left), ground(right)]))),
        1 => (0u8..4).prop_map(|tag| Some(compound(vec![ground(tag)]))),
        1 => (0i32..2, 0u8..4)
            .prop_map(|(level, tag)| Some(compound(vec![bound_level(level), ground(tag)]))),
        1 => Just(Some(CostSignature { value: None })),
        1 => Just(None),
    ]
}

fn arb_cell() -> impl Strategy<Value = CostSignature> {
    arb_signature().prop_map(|signature| signature.unwrap_or(CostSignature { value: None }))
}

fn arb_stack() -> impl Strategy<Value = CostStack> {
    prop::collection::vec(arb_cell(), 0..3).prop_map(|cells| CostStack { cells })
}

fn arb_expr() -> impl Strategy<Value = Expr> {
    prop_oneof![
        3 => (0i64..4).prop_map(|tag| Expr {
            expr_instance: Some(ExprInstance::GInt(tag)),
        }),
        1 => (0i32..2).prop_map(|level| dequotation(Some(VarInstance::BoundVar(level)))),
        1 => (0i32..2).prop_map(|level| dequotation(Some(VarInstance::FreeVar(level)))),
        1 => Just(dequotation(Some(VarInstance::Wildcard(WildcardMsg {})))),
        1 => Just(dequotation(None)),
        1 => Just(Expr {
            expr_instance: Some(ExprInstance::EVarBody(EVar { v: None })),
        }),
        1 => Just(Expr::default()),
    ]
}

fn leaf(sends: Vec<(i64, bool)>, exprs: Vec<Expr>, stacks: Vec<CostStack>) -> Par {
    let mut par = Par::default();
    par.sends.reserve_exact(sends.len());
    for (tag, persistent) in sends {
        par.sends.push(Send {
            chan: Some(channel(tag)),
            data: Vec::new(),
            persistent,
            locally_free: Vec::new(),
            connective_used: false,
        });
    }
    par.exprs = exprs;
    par.cost_stacks = stacks;
    par
}

fn arb_leaf() -> impl Strategy<Value = Par> {
    (
        prop::collection::vec((0i64..4, prop::bool::weighted(0.1)), 0..3),
        prop::collection::vec(arb_expr(), 0..2),
        prop::collection::vec(arb_stack(), 0..2),
    )
        .prop_map(|(sends, exprs, stacks)| leaf(sends, exprs, stacks))
}

type ArbSend = (i64, bool, Vec<Par>);
type ArbReceive = (Vec<(i64, Option<CostSignature>)>, bool, Option<Par>);
type ArbConditional = (Option<Par>, Option<Par>);

#[allow(clippy::too_many_arguments)]
fn node(
    signed: Vec<(Option<CostSignature>, Option<Par>)>,
    stacks: Vec<CostStack>,
    sends: Vec<ArbSend>,
    receives: Vec<ArbReceive>,
    news: Vec<Option<Par>>,
    matches: Vec<Vec<Option<Par>>>,
    conditionals: Vec<ArbConditional>,
    bundles: Vec<Option<Par>>,
    exprs: Vec<Expr>,
) -> Par {
    Par {
        cost_signed_terms: signed
            .into_iter()
            .map(|(signature, body)| CostSignedTerm { body, signature })
            .collect(),
        cost_stacks: stacks,
        sends: node_sends(sends),
        receives: node_receives(receives),
        news: node_news(news),
        matches: node_matches(matches),
        conditionals: node_conditionals(conditionals),
        bundles: node_bundles(bundles),
        exprs,
        ..Par::default()
    }
}

fn node_sends(sends: Vec<ArbSend>) -> Vec<Send> {
    sends
        .into_iter()
        .map(|(tag, persistent, data)| Send {
            chan: Some(channel(tag)),
            data,
            persistent,
            locally_free: Vec::new(),
            connective_used: false,
        })
        .collect()
}

fn node_receives(receives: Vec<ArbReceive>) -> Vec<Receive> {
    receives
        .into_iter()
        .map(|(binds, persistent, body)| {
            let bind_count = i32::try_from(binds.len()).expect("a generated receive has few binds");
            Receive {
                binds: binds
                    .into_iter()
                    .map(|(tag, cost_signature)| ReceiveBind {
                        patterns: Vec::new(),
                        source: Some(channel(tag)),
                        remainder: None,
                        free_count: 0,
                        cost_signature,
                    })
                    .collect(),
                body,
                persistent,
                peek: false,
                bind_count,
                locally_free: Vec::new(),
                connective_used: false,
                condition: None,
            }
        })
        .collect()
}

fn node_news(news: Vec<Option<Par>>) -> Vec<New> {
    news.into_iter()
        .map(|p| New {
            bind_count: 0,
            p,
            uri: Vec::new(),
            injections: BTreeMap::new(),
            locally_free: Vec::new(),
        })
        .collect()
}

fn node_matches(matches: Vec<Vec<Option<Par>>>) -> Vec<Match> {
    matches
        .into_iter()
        .map(|sources| Match {
            target: Some(Par::default()),
            cases: sources
                .into_iter()
                .map(|source| MatchCase {
                    pattern: Some(Par::default()),
                    source,
                    free_count: 0,
                    guard: None,
                })
                .collect(),
            locally_free: Vec::new(),
            connective_used: false,
        })
        .collect()
}

fn node_conditionals(conditionals: Vec<ArbConditional>) -> Vec<If> {
    conditionals
        .into_iter()
        .map(|(if_true, if_false)| If {
            condition: Some(Par::default()),
            if_true,
            if_false,
            locally_free: Vec::new(),
            connective_used: false,
        })
        .collect()
}

fn node_bundles(bundles: Vec<Option<Par>>) -> Vec<Bundle> {
    bundles
        .into_iter()
        .map(|body| Bundle {
            body,
            write_flag: true,
            read_flag: true,
        })
        .collect()
}

/// Random terms up to four levels deep with every construct the walks visit.
/// Absent optional children, persistent introductions, dequotations and
/// malformed signatures appear with small weights, so one term often carries
/// several rejection reasons and the first one decides the parity check.
fn arb_par() -> impl Strategy<Value = Par> {
    arb_leaf().prop_recursive(4, 64, 6, |inner| {
        let child = prop::option::weighted(0.85, inner.clone());
        (
            prop::collection::vec((arb_signature(), child.clone()), 0..3),
            prop::collection::vec(arb_stack(), 0..2),
            prop::collection::vec(
                (
                    0i64..4,
                    prop::bool::weighted(0.1),
                    prop::collection::vec(inner.clone(), 0..2),
                ),
                0..2,
            ),
            prop::collection::vec(
                (
                    prop::collection::vec((0i64..4, arb_signature()), 1..3),
                    prop::bool::weighted(0.1),
                    child.clone(),
                ),
                0..2,
            ),
            prop::collection::vec(child.clone(), 0..2),
            prop::collection::vec(prop::collection::vec(child.clone(), 0..3), 0..2),
            prop::collection::vec((child.clone(), child.clone()), 0..2),
            prop::collection::vec(child, 0..2),
            prop::collection::vec(arb_expr(), 0..2),
        )
            .prop_map(
                |(signed, stacks, sends, receives, news, matches, conditionals, bundles, exprs)| {
                    node(
                        signed,
                        stacks,
                        sends,
                        receives,
                        news,
                        matches,
                        conditionals,
                        bundles,
                        exprs,
                    )
                },
            )
    })
}

/// Compares every iterative walk with its recursive oracle on one term. The
/// `SignedDemand` comparison covers the lanes, the transfer lanes, the
/// guaranteed supply, the introduction flag and the first rejection reason.
/// The comparison of `static_authority_signatures` covers its first error.
fn assert_oracle_parity(par: &Par, deploy_key: SigKey) {
    for execution_position in [true, false] {
        let iterative = signed_demand_par(par, deploy_key, execution_position);
        let recursive =
            recursive_oracle::signed_demand_par(par, deploy_key, &[], execution_position);
        assert_eq!(
            iterative.unprovable, recursive.unprovable,
            "first rejection reason"
        );
        assert_eq!(iterative, recursive, "signed demand");
    }
    assert_eq!(
        static_authority_signatures(par),
        recursive_oracle::static_authority_signatures(par),
        "static authority signatures"
    );
    assert_eq!(
        demand_by_sig(par, deploy_key, &region_sig),
        recursive_oracle::demand_by_sig(par, deploy_key, &region_sig),
        "per-lane demand"
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn iterative_walks_match_the_recursive_oracle(par in arb_par(), deploy_tag in 0u8..4) {
        let deploy_key = [deploy_tag; 32];
        for execution_position in [true, false] {
            let iterative = signed_demand_par(&par, deploy_key, execution_position);
            let recursive =
                recursive_oracle::signed_demand_par(&par, deploy_key, &[], execution_position);
            prop_assert_eq!(&iterative.unprovable, &recursive.unprovable);
            prop_assert_eq!(iterative, recursive);
        }
        prop_assert_eq!(
            static_authority_signatures(&par),
            recursive_oracle::static_authority_signatures(&par)
        );
        prop_assert_eq!(
            demand_by_sig(&par, deploy_key, &region_sig),
            recursive_oracle::demand_by_sig(&par, deploy_key, &region_sig)
        );
    }
}

/// The first rejection reason follows the visit order of the recursion: signed
/// terms, stacks, sends, receives, `new`, `match`, `if`, bundles and then the
/// dequotation scan. Each case puts two reasons in a different order.
#[test]
fn first_rejection_reason_follows_the_recursive_visit_order() {
    let deploy_key = [1; 32];
    let dynamic_signed = |body: Par| {
        node(
            vec![(Some(bound_level(0)), Some(body))],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
    };
    let persistent_send = leaf(vec![(1, true)], Vec::new(), Vec::new());
    let dequoted = leaf(
        Vec::new(),
        vec![dequotation(Some(VarInstance::BoundVar(0)))],
        Vec::new(),
    );

    // A dynamic signed term precedes a persistent send in the same node.
    let mut signed_first = dynamic_signed(Par::default());
    signed_first.sends = persistent_send.sends.clone();
    // A persistent send precedes a dynamic signed term inside a bundle.
    let mut send_first = persistent_send.clone();
    send_first.bundles.push(Bundle {
        body: Some(dynamic_signed(Par::default())),
        write_flag: true,
        read_flag: true,
    });
    // The dequotation scan runs after every child of its node.
    let mut scan_last = dequoted.clone();
    scan_last.news.push(New {
        bind_count: 0,
        p: Some(persistent_send.clone()),
        uri: Vec::new(),
        injections: BTreeMap::new(),
        locally_free: Vec::new(),
    });

    for (par, expected) in [
        (&signed_first, UnprovableDemand::DynamicAuthority),
        (&send_first, UnprovableDemand::UnboundedControlFlow),
        (&scan_last, UnprovableDemand::UnboundedControlFlow),
        (&dequoted, UnprovableDemand::RecursiveDequotation),
    ] {
        assert_eq!(
            signed_demand_par(par, deploy_key, true).unprovable,
            Some(expected)
        );
        assert_oracle_parity(par, deploy_key);
    }
}

/// The Rholang sources of the repository: the examples, the interpreter test
/// resources and the system contracts. The walk over the directories uses a
/// heap work list.
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

/// Normalizes one corpus source. The external parser panics on syntax that it
/// does not implement (`select` in `examples/old/*/Cell2.rho`), so a panic
/// counts as a rejection, as an error does.
fn compile_corpus_source(source: &str) -> Option<Par> {
    use crate::rust::interpreter::compiler::compiler::Compiler;

    std::panic::catch_unwind(|| Compiler::source_to_adt(source))
        .ok()
        .and_then(Result::ok)
}

/// Oracle parity on every corpus program that the normalizer accepts. Some
/// sources need a deploy environment or use syntax that this normalizer
/// rejects, so the test requires a minimum number of compiled programs.
#[test]
fn iterative_walks_match_the_recursive_oracle_on_the_corpus() {
    let sources = corpus_sources();
    let mut compiled = 0usize;
    for (path, source) in &sources {
        // Changed after the first suite run of G1-1: the parser panicked on
        // `select`, so the compile goes through `compile_corpus_source`.
        // let Ok(par) = Compiler::source_to_adt(source) else {
        //     continue;
        // };
        let Some(par) = compile_corpus_source(source) else {
            continue;
        };
        compiled += 1;
        for deploy_tag in [1u8, 2] {
            let deploy_key = [deploy_tag; 32];
            for execution_position in [true, false] {
                assert_eq!(
                    signed_demand_par(&par, deploy_key, execution_position),
                    recursive_oracle::signed_demand_par(&par, deploy_key, &[], execution_position),
                    "signed demand of {}",
                    path.display()
                );
            }
            assert_eq!(
                demand_by_sig(&par, deploy_key, &region_sig),
                recursive_oracle::demand_by_sig(&par, deploy_key, &region_sig),
                "per-lane demand of {}",
                path.display()
            );
        }
        assert_eq!(
            static_authority_signatures(&par),
            recursive_oracle::static_authority_signatures(&par),
            "static authority signatures of {}",
            path.display()
        );
    }
    assert!(
        compiled >= 40,
        "only {compiled} of {} corpus sources compiled",
        sources.len()
    );
}

/// A `match` whose cases have no source and an `if` without branches add
/// nothing. The recursion combines an empty demand there, and the iterative
/// walk skips the combination, so both must agree.
#[test]
fn empty_alternatives_agree_with_the_oracle() {
    let par = node(
        vec![(
            Some(ground(3)),
            Some(leaf(vec![(2, false)], Vec::new(), Vec::new())),
        )],
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        vec![Vec::new(), vec![None, None]],
        vec![(None, None)],
        vec![None],
        Vec::new(),
    );
    assert_oracle_parity(&par, [7; 32]);
}

/// Builds a term whose process positions nest `depth` levels deep. Each level
/// holds one send and one construct that the walks descend into, in the cycle
/// signed term, `new`, bundle, `match`, `if` and receive. The loop wraps the
/// term built so far, so construction never recurses.
fn deep_term(depth: usize) -> Par {
    let mut par = leaf(vec![(0, false)], Vec::new(), Vec::new());
    for level in 0..depth {
        let mut outer = leaf(
            vec![(i64::try_from(level % 4).expect("small tag"), false)],
            Vec::new(),
            Vec::new(),
        );
        match level % 6 {
            0 => outer.cost_signed_terms.push(CostSignedTerm {
                body: Some(par),
                signature: Some(ground(u8::try_from(level / 6 % 4).expect("small tag"))),
            }),
            1 => outer.news.push(New {
                bind_count: 0,
                p: Some(par),
                uri: Vec::new(),
                injections: BTreeMap::new(),
                locally_free: Vec::new(),
            }),
            2 => outer.bundles.push(Bundle {
                body: Some(par),
                write_flag: true,
                read_flag: true,
            }),
            3 => outer.matches.push(Match {
                target: Some(Par::default()),
                cases: vec![MatchCase {
                    pattern: Some(Par::default()),
                    source: Some(par),
                    free_count: 0,
                    guard: None,
                }],
                locally_free: Vec::new(),
                connective_used: false,
            }),
            4 => outer.conditionals.push(If {
                condition: Some(Par::default()),
                if_true: Some(par),
                if_false: None,
                locally_free: Vec::new(),
                connective_used: false,
            }),
            _ => outer.receives.push(Receive {
                binds: vec![ReceiveBind {
                    patterns: Vec::new(),
                    source: Some(channel(2)),
                    remainder: None,
                    free_count: 0,
                    cost_signature: None,
                }],
                body: Some(par),
                persistent: false,
                peek: false,
                bind_count: 0,
                locally_free: Vec::new(),
                connective_used: false,
                condition: None,
            }),
        }
        par = outer;
    }
    par
}

/// Drops a deep term without recursion. The derived drop of prost recurses
/// once per nesting level, so each node first gives its children to a heap
/// work list. The node then holds no nested term and drops in constant stack.
fn dismantle(par: Par) {
    let mut pending = vec![par];
    while let Some(mut par) = pending.pop() {
        for term in par.cost_signed_terms.drain(..) {
            pending.extend(term.body);
        }
        for mut send in par.sends.drain(..) {
            pending.extend(send.chan.take());
            pending.append(&mut send.data);
        }
        for mut receive in par.receives.drain(..) {
            pending.extend(receive.body.take());
            pending.extend(receive.condition.take());
            for mut bind in receive.binds.drain(..) {
                pending.extend(bind.source.take());
                pending.append(&mut bind.patterns);
            }
        }
        for mut new in par.news.drain(..) {
            pending.extend(new.p.take());
        }
        for mut matched in par.matches.drain(..) {
            pending.extend(matched.target.take());
            for case in matched.cases.drain(..) {
                pending.extend(case.pattern);
                pending.extend(case.source);
                pending.extend(case.guard);
            }
        }
        for mut conditional in par.conditionals.drain(..) {
            pending.extend(conditional.condition.take());
            pending.extend(conditional.if_true.take());
            pending.extend(conditional.if_false.take());
        }
        for mut bundle in par.bundles.drain(..) {
            pending.extend(bundle.body.take());
        }
    }
}

struct DeepResults {
    demand: SignedDemand,
    signatures: Result<BTreeMap<SigKey, CostSignature>, AuthorityError>,
    lanes: BTreeMap<SigKey, DemandEntry>,
}

fn deep_results(par: &Par, deploy_key: SigKey) -> DeepResults {
    DeepResults {
        demand: signed_demand_par(par, deploy_key, true),
        signatures: static_authority_signatures(par),
        lanes: demand_by_sig(par, deploy_key, &region_sig),
    }
}

fn deep_oracle_results(par: &Par, deploy_key: SigKey) -> DeepResults {
    DeepResults {
        demand: recursive_oracle::signed_demand_par(par, deploy_key, &[], true),
        signatures: recursive_oracle::static_authority_signatures(par),
        lanes: recursive_oracle::demand_by_sig(par, deploy_key, &region_sig),
    }
}

/// The iterative walks finish on a term nested 100,000 levels deep on a
/// 256 KiB stack, and they agree with the recursive oracle, which needs a
/// stack that grows with the depth. Construction and teardown are iterative,
/// so the small-stack thread measures only the walks. The term is dismantled
/// before any assertion, so a failed assertion cannot drop it recursively.
#[test]
fn iterative_walks_run_a_term_nested_one_hundred_thousand_deep_on_a_small_stack() {
    let deploy_key = Sig::Ground(vec![9, 9, 9, 9]).lane_hash();
    let par = deep_term(DEEP_NESTING);
    let (iterative, recursive) = std::thread::scope(|scope| {
        let iterative = std::thread::Builder::new()
            .name("delta-sigma-small-stack".to_string())
            .stack_size(SMALL_STACK_BYTES)
            .spawn_scoped(scope, || deep_results(&par, deploy_key))
            .expect("spawn the small-stack thread")
            .join();
        let recursive = std::thread::Builder::new()
            .name("delta-sigma-oracle".to_string())
            .stack_size(ORACLE_STACK_BYTES)
            .spawn_scoped(scope, || deep_oracle_results(&par, deploy_key))
            .expect("spawn the oracle thread")
            .join();
        (iterative, recursive)
    });
    std::thread::Builder::new()
        .name("delta-sigma-teardown".to_string())
        .stack_size(SMALL_STACK_BYTES)
        .spawn(move || dismantle(par))
        .expect("spawn the teardown thread")
        .join()
        .expect("the iterative teardown stays within a small stack");
    let iterative = iterative.expect("the iterative walks stay within a small stack");
    let recursive = recursive.expect("the recursive oracle runs on a large stack");

    assert_eq!(iterative.demand.unprovable, None);
    assert!(iterative.demand.has_introduction);
    let introductions = iterative.demand.lanes.values().fold(0i64, |total, entry| {
        total.saturating_add(entry.certified_upper_bound)
    });
    assert!(
        introductions >= i64::try_from(DEEP_NESTING).expect("the depth fits in i64"),
        "every level holds a send"
    );
    assert_eq!(iterative.demand, recursive.demand);
    assert_eq!(
        iterative.signatures.as_ref().map(BTreeMap::len),
        Ok(4),
        "four ground signature tags"
    );
    assert_eq!(iterative.signatures, recursive.signatures);
    assert_eq!(iterative.lanes, recursive.lanes);
}
