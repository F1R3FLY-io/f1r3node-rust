//! # The sorter's recursive ORACLE, and the differential
//!
//! This source lives under `models/tests/support`; the production module tree
//! reaches it only through a `#[cfg(test)] #[path = ...]` declaration.
//!
//! Leg-2 Stage C-2 replaced the sorter's mutual recursion with an explicit heap
//! worklist ([`super::sort_drive`]). This module keeps the recursive traversal
//! alive as the reference the driver is compared against — exactly as
//! `reduce.rs` keeps `eval_expr_recursive` beside its trampoline (commit
//! `a929a2d6`), `models/tests/support/score_tree_oracle.rs` keeps
//! `compare_score_recursive`, and `rholang/tests/support/substitute_oracle.rs`
//! keeps its twin. It is cited rather than reinvented so a reviewer sees
//! the house standard.
//!
//! ## What this differential does and does not establish
//!
//! The oracle shares [`super::sort_combine`] with the driver, so the two differ
//! **only** in traversal shape. That is deliberate: it makes an *arm* bug
//! impossible to mistake for a *traversal* bug, and it makes the differential a
//! sharp test of the thing that actually changed.
//!
//! It follows that this differential **cannot** catch an error transcribed into
//! the shared table itself — both sides would be wrong together. That hole is
//! closed by `models/tests/sorter_canonical_golden.rs`, which pins the encoded
//! term and the score against a fixture captured from the **pre-conversion**
//! implementation. The two checks are complementary and neither is redundant:
//!
//! | check | catches |
//! |---|---|
//! | this module | a descent/`Combine` order error, a miscounted child slice, a missed node kind |
//! | the golden fixture | a mis-transcribed arm, a changed score shape, a changed field |
//! | `stack_depth_gate.rs` | the driver silently regaining a native-stack slope |
//!
//! ## ⚠ Why the corpus must carry multi-sibling terms
//!
//! A `Combine` that pops its children in the wrong order still produces a
//! same-*multiset* result. On a term with one child per slot that is
//! indistinguishable from correct. The corpus below therefore gives every
//! ordered slot at least **three** children with pairwise distinct scores, so
//! that any permutation is observable.

use super::score_tree::ScoredTerm;
use super::sort_combine::{
    combine_bind, combine_bundle, combine_case, combine_connective, combine_expr, combine_if,
    combine_match, combine_new, combine_par, combine_receive, combine_send, connective_child_pars,
    empty_par, expr_child_pars, sort_unforgeable, ParParts,
};
use crate::rhoapi::{
    Bundle, Connective, EPathMap, Expr, If, Match, MatchCase, New, Par, Receive, ReceiveBind, Send,
};
use crate::rust::canonical_path::decode_trie_path;
use crate::rust::epathmap_trie_codec::EPathMapMode;

// ===========================================================================
// the recursive traversal — the ORACLE
// ===========================================================================

/// `ParSortMatcher::sort_match`, recursively. Θ(depth) in native stack **on
/// purpose**: it is the reference, not the production path.
pub fn sort_par_recursive(par: &Par) -> ScoredTerm<Par> {
    let parts = ParParts {
        sends: par.sends.iter().map(sort_send_recursive).collect(),
        receives: par.receives.iter().map(sort_receive_recursive).collect(),
        exprs: par.exprs.iter().map(sort_expr_recursive).collect(),
        news: par.news.iter().map(sort_new_recursive).collect(),
        matches: par.matches.iter().map(sort_match_recursive).collect(),
        bundles: par.bundles.iter().map(sort_bundle_recursive).collect(),
        connectives: par
            .connectives
            .iter()
            .map(sort_connective_recursive)
            .collect(),
        unforgeables: par.unforgeables.iter().map(sort_unforgeable).collect(),
        conditionals: par.conditionals.iter().map(sort_if_recursive).collect(),
    };
    combine_par(par, parts)
}

pub fn sort_send_recursive(s: &Send) -> ScoredTerm<Send> {
    let chan = sort_par_recursive(
        s.chan
            .as_ref()
            .expect("channel field on Send was None, should be Some"),
    );
    let data = s.data.iter().map(sort_par_recursive).collect();
    combine_send(s, chan, data)
}

pub fn sort_bind_recursive(bind: &ReceiveBind) -> ScoredTerm<ReceiveBind> {
    let source = bind
        .source
        .as_ref()
        .expect("source field on Bind was None, should be Some");
    let patterns = bind.patterns.iter().map(sort_par_recursive).collect();
    let channel = sort_par_recursive(source);
    combine_bind(bind, patterns, channel)
}

pub fn sort_receive_recursive(r: &Receive) -> ScoredTerm<Receive> {
    let binds = r.binds.iter().map(sort_bind_recursive).collect();
    let body = sort_par_recursive(
        r.body
            .as_ref()
            .expect("body field on Receive was None, should be Some"),
    );
    let condition = sort_par_recursive(match r.condition.as_ref() {
        Some(p) => p,
        None => empty_par(),
    });
    combine_receive(r, binds, body, condition)
}

pub fn sort_new_recursive(n: &New) -> ScoredTerm<New> {
    let p = sort_par_recursive(
        n.p.as_ref()
            .expect("p field on New was None, should be Some"),
    );
    let injections = n.injections.values().map(sort_par_recursive).collect();
    combine_new(n, p, injections)
}

pub fn sort_case_recursive(case: &MatchCase) -> ScoredTerm<MatchCase> {
    let pattern = sort_par_recursive(
        case.pattern
            .as_ref()
            .expect("pattern field on MatchCase was None, should be Some"),
    );
    let source = sort_par_recursive(
        case.source
            .as_ref()
            .expect("source field on MatchCase was None, should be Some"),
    );
    let guard = sort_par_recursive(match case.guard.as_ref() {
        Some(p) => p,
        None => empty_par(),
    });
    combine_case(case, pattern, source, guard)
}

pub fn sort_match_recursive(m: &Match) -> ScoredTerm<Match> {
    let target = sort_par_recursive(
        m.target
            .as_ref()
            .expect("target field on Match was None, should be Some"),
    );
    let cases = m.cases.iter().map(sort_case_recursive).collect();
    combine_match(m, target, cases)
}

pub fn sort_if_recursive(i: &If) -> ScoredTerm<If> {
    let condition = sort_par_recursive(
        i.condition
            .as_ref()
            .expect("condition field on If was None, should be Some"),
    );
    let if_true = sort_par_recursive(
        i.if_true
            .as_ref()
            .expect("if_true field on If was None, should be Some"),
    );
    let if_false = sort_par_recursive(
        i.if_false
            .as_ref()
            .expect("if_false field on If was None, should be Some"),
    );
    combine_if(i, condition, if_true, if_false)
}

pub fn sort_bundle_recursive(b: &Bundle) -> ScoredTerm<Bundle> {
    let body = sort_par_recursive(b.body.as_ref().expect("body was None, should be Some(Par)"));
    combine_bundle(b, body)
}

pub fn sort_connective_recursive(c: &Connective) -> ScoredTerm<Connective> {
    let mut kids: Vec<&Par> = Vec::new();
    connective_child_pars(c, &mut kids);
    let scored = kids.into_iter().map(sort_par_recursive).collect();
    combine_connective(c, scored)
}

pub fn sort_expr_recursive(e: &Expr) -> ScoredTerm<Expr> {
    fn sort_pathmap_children(pathmap: &EPathMap) -> Vec<ScoredTerm<Par>> {
        let mut scored = Vec::with_capacity(match pathmap.mode() {
            EPathMapMode::Map => pathmap.len() * 2,
            EPathMapMode::Empty | EPathMapMode::Set => pathmap.len(),
        });
        match pathmap.mode() {
            EPathMapMode::Empty => {}
            EPathMapMode::Set => pathmap
                .entry_trie()
                .for_each_raw_set_entry(|key| {
                    let key = decode_trie_path(key)
                        .expect("set-mode EPathMap keys are canonical Par paths");
                    scored.push(sort_par_recursive(&key));
                })
                .expect("set-mode dispatch checked before traversal"),
            EPathMapMode::Map => pathmap
                .entry_trie()
                .for_each_raw_map_entry(|key, value| {
                    let key = decode_trie_path(key)
                        .expect("map-mode EPathMap keys are canonical Par paths");
                    scored.push(sort_par_recursive(&key));
                    scored.push(sort_par_recursive(value));
                })
                .expect("map-mode dispatch checked before traversal"),
        }
        scored
    }

    if let Some(instance) = e.expr_instance.as_ref() {
        match instance {
            crate::rhoapi::expr::ExprInstance::EPathmapBody(pathmap) => {
                return combine_expr(e, sort_pathmap_children(pathmap));
            }
            crate::rhoapi::expr::ExprInstance::EZipperBody(zipper) => {
                let pathmap = zipper.pathmap.as_ref().expect("zipper pathmap was None");
                return combine_expr(e, sort_pathmap_children(pathmap));
            }
            _ => {}
        }
    }
    let mut kids: Vec<&Par> = Vec::new();
    expr_child_pars(e, &mut kids);
    let scored = kids.into_iter().map(sort_par_recursive).collect();
    combine_expr(e, scored)
}

// ===========================================================================
// the differential
// ===========================================================================

#[cfg(test)]
mod differential_sorter {
    //! The obligation, discharged: for every corpus entry, the driver and the
    //! recursive oracle produce **the same term and the same score**.
    //!
    //! Nothing here compares a cost trace, because the sorter takes no metering
    //! handle and cannot charge — which makes the *result* obligation the whole
    //! obligation, and it is asserted on the encoded bytes (what `sig.rs`
    //! signs) rather than on a structural `==`.

    use std::collections::BTreeMap;

    use prost::Message;

    use super::*;
    use crate::rhoapi::connective::ConnectiveInstance;
    use crate::rhoapi::expr::ExprInstance;
    use crate::rhoapi::g_unforgeable::UnfInstance;
    use crate::rhoapi::var::VarInstance;
    use crate::rhoapi::{
        ConnectiveBody, EAnd, EDiv, EEq, EGt, EGte, EList, ELt, ELte, EMap, EMatches, EMethod,
        EMinus, EMinusMinus, EMod, EMult, ENeg, ENeq, ENot, EOr, EPathMap, EPercentPercent, EPlus,
        EPlusPlus, ESet, ETuple, EVar, EZipper, GBigRational, GDeployId, GDeployerId, GFixedPoint,
        GPrivate, GSysAuthToken, GUnforgeable, KeyValuePair, Var, VarRef,
    };
    use crate::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
    use crate::rust::rholang::sorter::receive_sort_matcher::ReceiveSortMatcher;
    use crate::rust::rholang::sorter::sortable::Sortable;

    fn gint(v: i64) -> Par {
        par_from_default! {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::GInt(v)),
            }],
            ..Default::default()
        }
    }

    fn gstring(v: &str) -> Par {
        par_from_default! {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::GString(v.to_string())),
            }],
            ..Default::default()
        }
    }

    fn expr_par(ei: ExprInstance) -> Par {
        par_from_default! {
            exprs: vec![Expr {
                expr_instance: Some(ei),
            }],
            ..Default::default()
        }
    }

    /// ⚠ Three children with pairwise distinct scores, in an order the sorter
    /// will have to change. A `Combine` that pops in the wrong order produces
    /// the same multiset and would be invisible on fewer.
    fn three() -> Vec<Par> { vec![gint(9), gstring("m"), gint(3)] }

    fn a() -> Option<Par> { Some(gint(1)) }
    fn b() -> Option<Par> { Some(gint(2)) }

    fn expr_instances() -> Vec<ExprInstance> {
        vec![
            ExprInstance::GBool(true),
            ExprInstance::GBool(false),
            ExprInstance::GInt(-7),
            ExprInstance::GString("zz".to_string()),
            ExprInstance::GUri("rho:io:stdout".to_string()),
            ExprInstance::GByteArray(vec![0x00, 0xff]),
            ExprInstance::GDouble(2.5f64.to_bits()),
            ExprInstance::GBigInt(vec![1, 2, 3]),
            ExprInstance::GBigRat(GBigRational {
                numerator: vec![3],
                denominator: vec![4],
            }),
            ExprInstance::GFixedPoint(GFixedPoint {
                unscaled: vec![9],
                scale: 3,
            }),
            ExprInstance::EVarBody(EVar {
                v: Some(Var {
                    var_instance: Some(VarInstance::BoundVar(3)),
                }),
            }),
            ExprInstance::EVarBody(EVar {
                v: Some(Var {
                    var_instance: Some(VarInstance::Wildcard(crate::rhoapi::var::WildcardMsg {})),
                }),
            }),
            ExprInstance::ENotBody(ENot { p: a() }),
            ExprInstance::ENegBody(ENeg { p: a() }),
            ExprInstance::EMultBody(EMult { p1: a(), p2: b() }),
            ExprInstance::EDivBody(EDiv { p1: a(), p2: b() }),
            ExprInstance::EModBody(EMod { p1: a(), p2: b() }),
            ExprInstance::EPlusBody(EPlus { p1: a(), p2: b() }),
            ExprInstance::EMinusBody(EMinus { p1: a(), p2: b() }),
            ExprInstance::EPlusPlusBody(EPlusPlus { p1: a(), p2: b() }),
            ExprInstance::EMinusMinusBody(EMinusMinus { p1: a(), p2: b() }),
            ExprInstance::EPercentPercentBody(EPercentPercent { p1: a(), p2: b() }),
            ExprInstance::ELtBody(ELt { p1: a(), p2: b() }),
            ExprInstance::ELteBody(ELte { p1: a(), p2: b() }),
            ExprInstance::EGtBody(EGt { p1: a(), p2: b() }),
            ExprInstance::EGteBody(EGte { p1: a(), p2: b() }),
            ExprInstance::EEqBody(EEq { p1: a(), p2: b() }),
            ExprInstance::ENeqBody(ENeq { p1: a(), p2: b() }),
            ExprInstance::EAndBody(EAnd { p1: a(), p2: b() }),
            ExprInstance::EOrBody(EOr { p1: a(), p2: b() }),
            ExprInstance::EMatchesBody(EMatches {
                target: a(),
                pattern: b(),
            }),
            ExprInstance::EListBody(EList {
                ps: three(),
                locally_free: vec![],
                connective_used: true,
                remainder: Some(Var {
                    var_instance: Some(VarInstance::FreeVar(0)),
                }),
            }),
            ExprInstance::EListBody(EList {
                ps: vec![],
                locally_free: vec![],
                connective_used: false,
                remainder: None,
            }),
            ExprInstance::ETupleBody(ETuple {
                ps: three(),
                locally_free: vec![],
                connective_used: false,
            }),
            ExprInstance::ESetBody(ESet {
                ps: three(),
                locally_free: vec![],
                connective_used: false,
                remainder: None,
            }),
            ExprInstance::EMapBody(EMap {
                kvs: vec![
                    KeyValuePair {
                        key: Some(gint(9)),
                        value: Some(gint(90)),
                    },
                    KeyValuePair {
                        key: Some(gint(3)),
                        value: Some(gint(30)),
                    },
                ],
                locally_free: vec![],
                connective_used: false,
                remainder: None,
            }),
            ExprInstance::EPathmapBody(EPathMap::new(three(), Vec::new(), false, None)),
            ExprInstance::EZipperBody(EZipper {
                pathmap: Some(EPathMap::new(three(), Vec::new(), false, None)),
                ..Default::default()
            }),
            ExprInstance::EMethodBody(EMethod {
                method_name: "nth".to_string(),
                target: Some(gint(5)),
                arguments: three(),
                locally_free: vec![],
                connective_used: true,
            }),
        ]
    }

    fn connective_instances() -> Vec<ConnectiveInstance> {
        vec![
            ConnectiveInstance::ConnAndBody(ConnectiveBody { ps: three() }),
            ConnectiveInstance::ConnOrBody(ConnectiveBody { ps: three() }),
            ConnectiveInstance::ConnNotBody(gint(4)),
            ConnectiveInstance::VarRefBody(VarRef { index: 1, depth: 2 }),
            ConnectiveInstance::ConnBool(true),
            ConnectiveInstance::ConnInt(false),
            ConnectiveInstance::ConnString(true),
            ConnectiveInstance::ConnUri(false),
            ConnectiveInstance::ConnByteArray(true),
        ]
    }

    fn send(persistent: bool) -> Send {
        Send {
            chan: Some(gstring("ch")),
            data: three(),
            persistent,
            locally_free: vec![1],
            connective_used: true,
        }
    }

    fn bind(with_remainder: bool) -> ReceiveBind {
        ReceiveBind {
            patterns: three(),
            source: Some(gstring("src")),
            remainder: if with_remainder {
                Some(Var {
                    var_instance: Some(VarInstance::FreeVar(2)),
                })
            } else {
                None
            },
            free_count: 2,
        }
    }

    fn receive(with_condition: bool) -> Receive {
        Receive {
            binds: vec![bind(true), bind(false)],
            body: Some(gint(7)),
            persistent: with_condition,
            peek: !with_condition,
            bind_count: 4,
            locally_free: vec![2],
            connective_used: false,
            condition: if with_condition { Some(gint(1)) } else { None },
        }
    }

    fn new_node() -> New {
        let mut injections = BTreeMap::new();
        injections.insert("zeta".to_string(), gint(9));
        injections.insert("alpha".to_string(), gint(3));
        injections.insert("mid".to_string(), gstring("m"));
        New {
            bind_count: 2,
            p: Some(gint(7)),
            uri: vec!["rho:z".to_string(), "rho:a".to_string()],
            injections,
            locally_free: vec![3],
        }
    }

    fn match_node() -> Match {
        Match {
            target: Some(gstring("t")),
            cases: vec![
                MatchCase {
                    pattern: Some(gint(9)),
                    source: Some(gint(3)),
                    free_count: 1,
                    guard: Some(gint(5)),
                },
                MatchCase {
                    pattern: Some(gstring("p")),
                    source: Some(gint(1)),
                    free_count: 0,
                    guard: None,
                },
            ],
            locally_free: vec![4],
            connective_used: true,
        }
    }

    fn if_node() -> If {
        If {
            condition: Some(gint(1)),
            if_true: Some(gint(2)),
            if_false: Some(gstring("f")),
            locally_free: vec![5],
            connective_used: false,
        }
    }

    fn bundle(write_flag: bool, read_flag: bool) -> Bundle {
        Bundle {
            body: Some(gint(6)),
            write_flag,
            read_flag,
        }
    }

    /// A `Par` with **every** category populated and several members in each,
    /// which is what makes an out-of-order `Combine` observable.
    fn full_par() -> Par {
        Par {
            sends: vec![send(true), send(false)],
            receives: vec![receive(true), receive(false)],
            news: vec![new_node()],
            exprs: vec![
                Expr {
                    expr_instance: Some(ExprInstance::GInt(9)),
                },
                Expr {
                    expr_instance: Some(ExprInstance::GString("e".to_string())),
                },
                Expr {
                    expr_instance: Some(ExprInstance::GInt(3)),
                },
            ],
            matches: vec![match_node()],
            unforgeables: vec![
                GUnforgeable {
                    unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id: vec![9] })),
                },
                GUnforgeable {
                    unf_instance: Some(UnfInstance::GDeployerIdBody(GDeployerId {
                        public_key: vec![1],
                    })),
                },
                GUnforgeable {
                    unf_instance: Some(UnfInstance::GDeployIdBody(GDeployId { sig: vec![2] })),
                },
                GUnforgeable {
                    unf_instance: Some(UnfInstance::GSysAuthTokenBody(GSysAuthToken {})),
                },
            ],
            bundles: vec![bundle(true, false), bundle(false, true), bundle(true, true)],
            connectives: vec![
                Connective {
                    connective_instance: Some(ConnectiveInstance::ConnBool(true)),
                },
                Connective {
                    connective_instance: Some(ConnectiveInstance::ConnInt(true)),
                },
                Connective {
                    connective_instance: Some(ConnectiveInstance::ConnString(true)),
                },
            ],
            conditionals: vec![if_node()],
            locally_free: vec![1, 2, 3],
            connective_used: true,
        }
    }

    /// `[[[…]]]` with a distinct sibling at every level, so the descent order
    /// is observable at depth as well as at width.
    fn nested(depth: usize) -> Par {
        let mut p = gint(0);
        for i in 0..depth {
            p = expr_par(ExprInstance::EListBody(EList {
                ps: vec![p, gint(i as i64 + 10), gstring("s")],
                locally_free: vec![],
                connective_used: false,
                remainder: None,
            }));
        }
        p
    }

    fn nested_set(depth: usize) -> Par {
        let mut p = gint(0);
        for _ in 0..depth {
            p = expr_par(ExprInstance::ESetBody(ESet {
                ps: vec![p],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            }));
        }
        p
    }

    fn nested_map(depth: usize) -> Par {
        let mut p = gint(0);
        for _ in 0..depth {
            p = expr_par(ExprInstance::EMapBody(EMap {
                kvs: vec![KeyValuePair {
                    key: Some(p),
                    value: Some(gint(1)),
                }],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            }));
        }
        p
    }

    fn par_corpus() -> Vec<Par> {
        let mut out: Vec<Par> = Vec::new();
        out.push(Par::default());
        out.push(full_par());
        for depth in [1usize, 2, 3, 5] {
            out.push(nested(depth));
        }
        for ei in expr_instances() {
            out.push(expr_par(ei));
        }
        for ci in connective_instances() {
            out.push(par_from_default! {
                connectives: vec![Connective {
                    connective_instance: Some(ci),
                }],
                ..Default::default()
            });
        }
        out.push(par_from_default! {
            exprs: vec![Expr {
                expr_instance: None,
            }],
            ..Default::default()
        });
        out.push(par_from_default! {
            connectives: vec![Connective {
                connective_instance: None,
            }],
            ..Default::default()
        });
        out.push(par_from_default! {
            unforgeables: vec![GUnforgeable { unf_instance: None }],
            ..Default::default()
        });
        // Every `Par` above, once more wrapped in a `Send` / `Receive` / `New`
        // / `Match` / `Bundle`, so that each non-`Par` node kind is reached at
        // depth rather than only at the root.
        let wrapped: Vec<Par> = out
            .iter()
            .take(12)
            .map(|p| {
                par_from_default! {
                    sends: vec![Send {
                        chan: Some(p.clone()),
                        data: vec![p.clone(), gint(3)],
                        persistent: false,
                        locally_free: vec![],
                        connective_used: false,
                    }],
                    bundles: vec![Bundle {
                        body: Some(p.clone()),
                        write_flag: true,
                        read_flag: false,
                    }],
                    ..Default::default()
                }
            })
            .collect();
        out.extend(wrapped);
        out
    }

    /// The assertion every entry gets: same encoded term, same rendered score.
    fn same<T: Message + PartialEq + std::fmt::Debug>(
        what: &str,
        driver: ScoredTerm<T>,
        oracle: ScoredTerm<T>,
    ) {
        assert_eq!(
            driver.term.encode_to_vec(),
            oracle.term.encode_to_vec(),
            "★ THE CANONICAL FORM DIVERGED between the worklist driver and the recursive \
             oracle for {what}. `cost_accounting/sig.rs` signs \
             `sort_match(&par).term.encode_to_vec()`, so this is a consensus fork.\n\
             driver = {:?}\noracle = {:?}",
            driver.term,
            oracle.term
        );
        assert!(
            driver.score == oracle.score,
            "the SCORE diverged between the worklist driver and the recursive oracle for \
             {what}; a score difference reorders siblings, which changes the signed \
             bytes.\ndriver = {:?}\noracle = {:?}",
            driver.score,
            oracle.score
        );
    }

    #[test]
    fn the_worklist_driver_agrees_with_the_recursive_oracle_on_every_par() {
        for (i, p) in par_corpus().iter().enumerate() {
            same(
                &format!("par_corpus[{i}]"),
                ParSortMatcher::sort_match(p),
                sort_par_recursive(p),
            );
        }
    }

    #[test]
    fn the_worklist_driver_agrees_with_the_recursive_oracle_on_every_node_kind() {
        for persistent in [true, false] {
            same(
                "send",
                super::super::sort_drive::sort_send(&send(persistent)),
                sort_send_recursive(&send(persistent)),
            );
        }
        for with_condition in [true, false] {
            same(
                "receive",
                super::super::sort_drive::sort_receive(&receive(with_condition)),
                sort_receive_recursive(&receive(with_condition)),
            );
        }
        for with_remainder in [true, false] {
            let driver = ReceiveSortMatcher::sort_bind(bind(with_remainder));
            let oracle = sort_bind_recursive(&bind(with_remainder));
            assert_eq!(
                driver.term, oracle.term,
                "the ReceiveBind term diverged between driver and oracle"
            );
            assert!(
                driver.score == oracle.score,
                "the ReceiveBind score diverged between driver and oracle"
            );
        }
        same(
            "new",
            super::super::sort_drive::sort_new(&new_node()),
            sort_new_recursive(&new_node()),
        );
        same(
            "match",
            super::super::sort_drive::sort_match_node(&match_node()),
            sort_match_recursive(&match_node()),
        );
        same(
            "if",
            super::super::sort_drive::sort_if(&if_node()),
            sort_if_recursive(&if_node()),
        );
        for (w, r) in [(false, false), (false, true), (true, false), (true, true)] {
            same(
                "bundle",
                super::super::sort_drive::sort_bundle(&bundle(w, r)),
                sort_bundle_recursive(&bundle(w, r)),
            );
        }
        for ci in connective_instances() {
            let c = Connective {
                connective_instance: Some(ci),
            };
            same(
                "connective",
                super::super::sort_drive::sort_connective(&c),
                sort_connective_recursive(&c),
            );
        }
        for ei in expr_instances() {
            let e = Expr {
                expr_instance: Some(ei),
            };
            same(
                "expr",
                super::super::sort_drive::sort_expr(&e),
                sort_expr_recursive(&e),
            );
        }
    }

    /// ⚠ The check the ordinary corpus cannot make: a `Combine` that popped
    /// its children in the wrong order still yields the same multiset. Here the
    /// slots are **order-preserving** (`EList`, `ETuple`, `Send::data`,
    /// `EMethod::arguments`, `ReceiveBind::patterns`), so the output order IS
    /// the input order and a permutation is a hard failure.
    #[test]
    fn order_preserving_slots_keep_their_input_order() {
        let elems = vec![gint(9), gstring("m"), gint(3), gint(1)];

        let list = expr_par(ExprInstance::EListBody(EList {
            ps: elems.clone(),
            locally_free: vec![],
            connective_used: false,
            remainder: None,
        }));
        let sorted = ParSortMatcher::sort_match(&list).term;
        let got = match sorted.exprs[0]
            .expr_instance
            .as_ref()
            .expect("EList survived sorting")
        {
            ExprInstance::EListBody(l) => l.ps.clone(),
            other => panic!("EList became {:?}", other),
        };
        assert_eq!(
            got, elems,
            "★ `EList` is ORDER-PRESERVING and its elements moved. A `Combine` that pops \
             its children in the wrong order produces the same multiset, so this is the \
             assertion that catches a reversal — and the canonical form is signed."
        );

        let s = Send {
            chan: Some(gstring("ch")),
            data: elems.clone(),
            persistent: false,
            locally_free: vec![],
            connective_used: false,
        };
        let sorted_send = super::super::sort_drive::sort_send(&s).term;
        assert_eq!(
            sorted_send.data, elems,
            "★ `Send::data` is ORDER-PRESERVING and its elements moved."
        );

        let m = EMethod {
            method_name: "nth".to_string(),
            target: Some(gint(5)),
            arguments: elems.clone(),
            locally_free: vec![],
            connective_used: false,
        };
        let sorted_method = super::super::sort_drive::sort_expr(&Expr {
            expr_instance: Some(ExprInstance::EMethodBody(m)),
        })
        .term;
        match sorted_method
            .expr_instance
            .as_ref()
            .expect("EMethod survived sorting")
        {
            ExprInstance::EMethodBody(em) => {
                assert_eq!(
                    em.arguments, elems,
                    "★ `EMethod::arguments` is ORDER-PRESERVING and its elements moved."
                );
                assert_eq!(
                    em.target.as_ref(),
                    Some(&gint(5)),
                    "★ `EMethod`'s TARGET was confused with an argument — the slot list \
                     pushes arguments FIRST and the target LAST."
                );
            }
            other => panic!("EMethod became {:?}", other),
        }

        let rb = ReceiveBind {
            patterns: elems.clone(),
            source: Some(gstring("src")),
            remainder: None,
            free_count: 0,
        };
        let sorted_bind = ReceiveSortMatcher::sort_bind(rb).term;
        assert_eq!(
            sorted_bind.patterns, elems,
            "★ `ReceiveBind::patterns` is ORDER-PRESERVING and its elements moved."
        );
        assert_eq!(
            sorted_bind.source.as_ref(),
            Some(&gstring("src")),
            "★ `ReceiveBind`'s SOURCE was confused with a pattern."
        );
    }

    /// The driver must not need the native stack that the oracle does. 20,000
    /// levels is far past what the pre-conversion sorter survived on any
    /// ordinary thread (78,579 B/level debug ⇒ ~1.5 GiB).
    #[test]
    fn the_driver_survives_a_depth_the_oracle_could_not() {
        let deep = nested(20_000);
        let sorted = ParSortMatcher::sort_match(&deep);
        assert!(
            !sorted.term.exprs.is_empty(),
            "the driver produced an empty Par for a 20,000-deep chain"
        );
        // Teardown is `drop_in_place`, which is still recursive (audit row 7 —
        // the irreducible member), so the terms are dismantled iteratively.
        crate::rust::rholang::par_children::dismantle(sorted.term);
        crate::rust::rholang::par_children::dismantle(deep);
    }

    #[test]
    fn nested_set_and_map_sorting_are_stack_safe_on_a_256_kib_stack() {
        std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(|| {
                for deep in [nested_set(4_096), nested_map(4_096)] {
                    let sorted = ParSortMatcher::sort_match(&deep);
                    assert_eq!(sorted.term.exprs.len(), 1);
                    crate::rust::rholang::par_children::dismantle(sorted.term);
                    crate::rust::rholang::par_children::dismantle(deep);
                }
            })
            .expect("spawn the small-stack collection-sort probe")
            .join()
            .expect("collection sorting must not overflow the 256 KiB stack");
    }
}
