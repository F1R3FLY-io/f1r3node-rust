use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use super::ordering::Ordering;
use super::score_tree::{Score, ScoreAtom, ScoredTerm, Tree};
use crate::rhoapi::connective::ConnectiveInstance;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::var::VarInstance;
use crate::rhoapi::{
    Bundle, Connective, CostSignature, CostSignedTerm, CostStack, EAnd, EDiv, EEq, EGt, EGte,
    EList, ELt, ELte, EMap, EMatches, EMethod, EMinus, EMinusMinus, EMod, EMult, ENeg, ENeq, ENot,
    EOr, EPathMap, EPercentPercent, EPlus, EPlusPlus, ESet, ETuple, EVar, EZipper, Expr,
    GBigRational, GFixedPoint, GUnforgeable, If, KeyValuePair, Match, MatchCase, New, Par, Receive,
    ReceiveBind, Send, Var,
};
use crate::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use crate::rust::rholang::sorter::sortable::Sortable;

pub fn split_scored_terms<T>(scored_terms: Vec<ScoredTerm<T>>) -> (Vec<T>, Vec<Tree<ScoreAtom>>) {
    let mut terms = Vec::with_capacity(scored_terms.len());
    let mut scores = Vec::with_capacity(scored_terms.len());

    for scored in scored_terms {
        terms.push(scored.term);
        scores.push(scored.score);
    }

    (terms, scores)
}

pub fn empty_par() -> &'static Par {
    static EMPTY: OnceLock<Par> = OnceLock::new();
    EMPTY.get_or_init(Par::default)
}

fn flag_score(b: bool) -> i64 {
    if b {
        1
    } else {
        0
    }
}

fn remainder_score(remainder: &Option<Var>) -> Tree<ScoreAtom> {
    match remainder {
        Some(v) => sort_var(v).score,
        None => Tree::<ScoreAtom>::create_leaf_from_i64(-1),
    }
}

pub fn sort_var(v: &Var) -> ScoredTerm<Var> {
    match &v.var_instance {
        Some(var) => match var {
            VarInstance::BoundVar(level) => ScoredTerm {
                term: v.clone(),
                score: Tree::<ScoreAtom>::create_node_from_i64s(vec![
                    Score::BOUND_VAR as i64,
                    *level as i64,
                ]),
            },

            VarInstance::FreeVar(level) => ScoredTerm {
                term: v.clone(),
                score: Tree::<ScoreAtom>::create_node_from_i64s(vec![
                    Score::FREE_VAR as i64,
                    *level as i64,
                ]),
            },

            VarInstance::Wildcard(_) => ScoredTerm {
                term: v.clone(),
                score: Tree::<ScoreAtom>::create_node_from_i64s(vec![Score::WILDCARD as i64]),
            },
        },
        None => ScoredTerm {
            term: Var::default(),
            score: Tree::<ScoreAtom>::create_leaf_from_i64(Score::ABSENT as i64),
        },
    }
}

pub fn sort_unforgeable(value: &GUnforgeable) -> ScoredTerm<GUnforgeable> {
    super::unforgeable_sort_matcher::UnforgeableSortMatcher::sort_match(value)
}

pub fn expr_child_pars<'t>(e: &'t Expr, out: &mut Vec<&'t Par>) {
    let Some(instance) = e.expr_instance.as_ref() else {
        return;
    };
    match instance {
        ExprInstance::GBool(_)
        | ExprInstance::GInt(_)
        | ExprInstance::GString(_)
        | ExprInstance::GUri(_)
        | ExprInstance::GByteArray(_)
        | ExprInstance::GDouble(_)
        | ExprInstance::GBigInt(_)
        | ExprInstance::GBigRat(_)
        | ExprInstance::GFixedPoint(_)
        | ExprInstance::GFloat32(_)
        | ExprInstance::GUint64(_)
        | ExprInstance::GInt32(_)
        | ExprInstance::GUint32(_)
        | ExprInstance::GUint16(_)
        | ExprInstance::GUint8(_)
        | ExprInstance::EVarBody(_) => {}

        ExprInstance::ESetBody(_) | ExprInstance::EMapBody(_) => {}

        ExprInstance::ENegBody(x) => out.push(required(&x.p)),
        ExprInstance::ENotBody(x) => out.push(required(&x.p)),

        ExprInstance::EMultBody(x) => {
            out.push(required(&x.p1));
            out.push(required(&x.p2));
        }
        ExprInstance::EDivBody(x) => {
            out.push(required(&x.p1));
            out.push(required(&x.p2));
        }
        ExprInstance::EModBody(x) => {
            out.push(required(&x.p1));
            out.push(required(&x.p2));
        }
        ExprInstance::EPlusBody(x) => {
            out.push(required(&x.p1));
            out.push(required(&x.p2));
        }
        ExprInstance::EMinusBody(x) => {
            out.push(required(&x.p1));
            out.push(required(&x.p2));
        }
        ExprInstance::ELtBody(x) => {
            out.push(required(&x.p1));
            out.push(required(&x.p2));
        }
        ExprInstance::ELteBody(x) => {
            out.push(required(&x.p1));
            out.push(required(&x.p2));
        }
        ExprInstance::EGtBody(x) => {
            out.push(required(&x.p1));
            out.push(required(&x.p2));
        }
        ExprInstance::EGteBody(x) => {
            out.push(required(&x.p1));
            out.push(required(&x.p2));
        }
        ExprInstance::EEqBody(x) => {
            out.push(required(&x.p1));
            out.push(required(&x.p2));
        }
        ExprInstance::ENeqBody(x) => {
            out.push(required(&x.p1));
            out.push(required(&x.p2));
        }
        ExprInstance::EAndBody(x) => {
            out.push(required(&x.p1));
            out.push(required(&x.p2));
        }
        ExprInstance::EOrBody(x) => {
            out.push(required(&x.p1));
            out.push(required(&x.p2));
        }
        ExprInstance::EPercentPercentBody(x) => {
            out.push(required(&x.p1));
            out.push(required(&x.p2));
        }
        ExprInstance::EPlusPlusBody(x) => {
            out.push(required(&x.p1));
            out.push(required(&x.p2));
        }
        ExprInstance::EMinusMinusBody(x) => {
            out.push(required(&x.p1));
            out.push(required(&x.p2));
        }
        ExprInstance::EMatchesBody(x) => {
            out.push(
                x.target
                    .as_ref()
                    .expect("target field was None, should be Some"),
            );
            out.push(
                x.pattern
                    .as_ref()
                    .expect("pattern field was None, should be Some"),
            );
        }

        ExprInstance::EPathmapBody(x) => out.extend(x.ps.iter()),
        ExprInstance::EListBody(x) => out.extend(x.ps.iter()),
        ExprInstance::ETupleBody(x) => out.extend(x.ps.iter()),
        ExprInstance::EZipperBody(x) => out.extend(
            x.pathmap
                .as_ref()
                .expect("zipper pathmap was None")
                .ps
                .iter(),
        ),

        ExprInstance::EMethodBody(x) => {
            out.extend(x.arguments.iter());
            out.push(
                x.target
                    .as_ref()
                    .expect("target field on EMethod was None, should be Some"),
            );
        }
    }
}

fn required(slot: &Option<Par>) -> &Par {
    slot.as_ref().expect("par field was None, should be Some")
}

pub fn connective_child_pars<'t>(c: &'t Connective, out: &mut Vec<&'t Par>) {
    match &c.connective_instance {
        Some(ConnectiveInstance::ConnAndBody(cb)) => out.extend(cb.ps.iter()),
        Some(ConnectiveInstance::ConnOrBody(cb)) => out.extend(cb.ps.iter()),
        Some(ConnectiveInstance::ConnNotBody(p)) => out.push(p),
        Some(ConnectiveInstance::VarRefBody(_))
        | Some(ConnectiveInstance::ConnBool(_))
        | Some(ConnectiveInstance::ConnInt(_))
        | Some(ConnectiveInstance::ConnString(_))
        | Some(ConnectiveInstance::ConnUri(_))
        | Some(ConnectiveInstance::ConnByteArray(_))
        | None => {}
    }
}

pub struct ParParts {
    pub sends: Vec<ScoredTerm<Send>>,
    pub receives: Vec<ScoredTerm<Receive>>,
    pub exprs: Vec<ScoredTerm<Expr>>,
    pub news: Vec<ScoredTerm<New>>,
    pub matches: Vec<ScoredTerm<Match>>,
    pub bundles: Vec<ScoredTerm<Bundle>>,
    pub connectives: Vec<ScoredTerm<Connective>>,
    pub unforgeables: Vec<ScoredTerm<GUnforgeable>>,
    pub conditionals: Vec<ScoredTerm<If>>,
    pub cost_signed_terms: Vec<ScoredTerm<CostSignedTerm>>,
    pub cost_stacks: Vec<ScoredTerm<CostStack>>,
}

pub fn combine_par(par: &Par, parts: ParParts) -> ScoredTerm<Par> {
    let ParParts {
        mut sends,
        mut receives,
        mut exprs,
        mut news,
        mut matches,
        mut bundles,
        mut connectives,
        mut unforgeables,
        mut conditionals,
        mut cost_signed_terms,
        mut cost_stacks,
    } = parts;

    ScoredTerm::sort_vec(&mut sends);
    ScoredTerm::sort_vec(&mut receives);
    ScoredTerm::sort_vec(&mut exprs);
    ScoredTerm::sort_vec(&mut news);
    ScoredTerm::sort_vec(&mut matches);
    ScoredTerm::sort_vec(&mut bundles);
    ScoredTerm::sort_vec(&mut connectives);
    ScoredTerm::sort_vec(&mut unforgeables);
    ScoredTerm::sort_vec(&mut conditionals);
    ScoredTerm::sort_vec(&mut cost_signed_terms);
    ScoredTerm::sort_vec(&mut cost_stacks);

    let (send_terms, send_scores) = split_scored_terms(sends);
    let (receive_terms, receive_scores) = split_scored_terms(receives);
    let (news_terms, news_scores) = split_scored_terms(news);
    let (expr_terms, expr_scores) = split_scored_terms(exprs);
    let (match_terms, match_scores) = split_scored_terms(matches);
    let (bundle_terms, bundle_scores) = split_scored_terms(bundles);
    let (connective_terms, connective_scores) = split_scored_terms(connectives);
    let (unforgeable_terms, unforgeable_scores) = split_scored_terms(unforgeables);
    let (conditional_terms, conditional_scores) = split_scored_terms(conditionals);
    let (cost_signed_terms, cost_signed_scores) = split_scored_terms(cost_signed_terms);
    let (cost_stacks, cost_stack_scores) = split_scored_terms(cost_stacks);

    let sorted_par = Par {
        sends: send_terms,
        receives: receive_terms,
        news: news_terms,
        exprs: expr_terms,
        matches: match_terms,
        unforgeables: unforgeable_terms,
        bundles: bundle_terms,
        connectives: connective_terms,
        conditionals: conditional_terms,
        cost_signed_terms,
        cost_stacks,
        locally_free: par.locally_free.clone(),
        connective_used: par.connective_used,
    };

    let par_score = Tree::<ScoreAtom>::create_node_from_i32(
        Score::PAR,
        send_scores
            .into_iter()
            .chain(receive_scores)
            .chain(expr_scores)
            .chain(news_scores)
            .chain(match_scores)
            .chain(bundle_scores)
            .chain(connective_scores)
            .chain(unforgeable_scores)
            .chain(conditional_scores)
            .chain(cost_signed_scores)
            .chain(cost_stack_scores)
            .chain(std::iter::once(Tree::<ScoreAtom>::create_leaf_from_i64(
                flag_score(par.connective_used),
            )))
            .collect(),
    );

    ScoredTerm {
        term: sorted_par,
        score: par_score,
    }
}

pub fn combine_send(
    s: &Send,
    chan: ScoredTerm<Par>,
    data: Vec<ScoredTerm<Par>>,
) -> ScoredTerm<Send> {
    let (data_terms, data_scores) = split_scored_terms(data);

    let sorted_send = Send {
        chan: Some(chan.term),
        data: data_terms,
        persistent: s.persistent,
        locally_free: s.locally_free.clone(),
        connective_used: s.connective_used,
    };

    let send_score = Tree::<ScoreAtom>::create_node_from_i32(
        Score::SEND,
        vec![
            Tree::<ScoreAtom>::create_leaf_from_i64(flag_score(s.persistent)),
            chan.score,
        ]
        .into_iter()
        .chain(data_scores)
        .chain(std::iter::once(Tree::<ScoreAtom>::create_leaf_from_i64(
            flag_score(s.connective_used),
        )))
        .collect(),
    );

    ScoredTerm {
        term: sorted_send,
        score: send_score,
    }
}

pub fn combine_bind(
    bind: &ReceiveBind,
    patterns: Vec<ScoredTerm<Par>>,
    channel: ScoredTerm<Par>,
    cost_signature: Option<ScoredTerm<CostSignature>>,
) -> ScoredTerm<ReceiveBind> {
    let sorted_remainder_score = match &bind.remainder {
        Some(bind_remainder) => sort_var(bind_remainder).score,
        None => Tree::<ScoreAtom>::create_leaf_from_i64(Score::ABSENT as i64),
    };

    let (pattern_terms, pattern_scores) = split_scored_terms(patterns);
    let (cost_signature, cost_score) = match cost_signature {
        Some(signature) => (Some(signature.term), Some(signature.score)),
        None => (None, None),
    };

    ScoredTerm {
        term: ReceiveBind {
            patterns: pattern_terms,
            source: Some(channel.term),
            remainder: bind.remainder.clone(),
            free_count: bind.free_count,
            cost_signature,
        },
        score: Tree::Node(
            std::iter::once(channel.score)
                .chain(pattern_scores)
                .chain(std::iter::once(sorted_remainder_score))
                .chain(cost_score)
                .collect(),
        ),
    }
}

pub fn combine_receive(
    r: &Receive,
    binds: Vec<ScoredTerm<ReceiveBind>>,
    body: ScoredTerm<Par>,
    condition: ScoredTerm<Par>,
) -> ScoredTerm<Receive> {
    let condition_present = r.condition.as_ref().is_some_and(|p| p != &Par::default());
    let ScoredTerm {
        term: condition_term,
        score: condition_score,
    } = condition;
    let condition_term = if condition_present {
        Some(condition_term)
    } else {
        None
    };

    let (bind_terms, bind_scores) = split_scored_terms(binds);

    ScoredTerm {
        term: Receive {
            binds: bind_terms,
            body: Some(body.term),
            persistent: r.persistent,
            peek: r.peek,
            bind_count: r.bind_count,
            locally_free: r.locally_free.clone(),
            connective_used: r.connective_used,
            condition: condition_term,
        },
        score: Tree::<ScoreAtom>::create_node_from_i32(
            Score::RECEIVE,
            vec![
                Tree::<ScoreAtom>::create_leaf_from_i64(flag_score(r.persistent)),
                Tree::<ScoreAtom>::create_leaf_from_i64(flag_score(r.peek)),
            ]
            .into_iter()
            .chain(bind_scores)
            .chain(std::iter::once(body.score))
            .chain(vec![
                Tree::<ScoreAtom>::create_leaf_from_i64(r.bind_count as i64),
                Tree::<ScoreAtom>::create_leaf_from_i64(flag_score(r.connective_used)),
                condition_score,
            ])
            .collect(),
        ),
    }
}

pub fn combine_new(
    n: &New,
    p: ScoredTerm<Par>,
    injections: Vec<ScoredTerm<Par>>,
) -> ScoredTerm<New> {
    let mut sorted_uri = n.uri.clone();
    sorted_uri.sort();

    let uri_score: Vec<Tree<ScoreAtom>> = if !sorted_uri.is_empty() {
        sorted_uri
            .iter()
            .map(|s| Tree::<ScoreAtom>::create_leaf_from_string(s.clone()))
            .collect()
    } else {
        vec![Tree::<ScoreAtom>::create_leaf_from_i64(
            Score::ABSENT as i64,
        )]
    };

    let injections_score: Vec<Tree<ScoreAtom>> = if !n.injections.is_empty() {
        n.injections
            .keys()
            .zip(injections)
            .map(|(k, scored)| {
                Tree::<ScoreAtom>::create_node_from_string(k.clone(), vec![scored.score])
            })
            .collect()
    } else {
        vec![Tree::<ScoreAtom>::create_leaf_from_i64(
            Score::ABSENT as i64,
        )]
    };

    ScoredTerm {
        term: New {
            bind_count: n.bind_count,
            p: Some(p.term),
            uri: sorted_uri,
            injections: n.injections.clone(),
            locally_free: n.locally_free.clone(),
        },
        score: Tree::Node(
            std::iter::once(Tree::<ScoreAtom>::create_leaf_from_i64(Score::NEW as i64))
                .chain(std::iter::once(Tree::<ScoreAtom>::create_leaf_from_i64(
                    n.bind_count as i64,
                )))
                .chain(uri_score)
                .chain(injections_score)
                .chain(std::iter::once(p.score))
                .collect(),
        ),
    }
}

pub fn combine_case(
    match_case: &MatchCase,
    pattern: ScoredTerm<Par>,
    source: ScoredTerm<Par>,
    guard: ScoredTerm<Par>,
) -> ScoredTerm<MatchCase> {
    let guard_present = match_case
        .guard
        .as_ref()
        .is_some_and(|p| p != &Par::default());
    let ScoredTerm {
        term: guard_term,
        score: guard_score,
    } = guard;
    let guard_term = if guard_present {
        Some(guard_term)
    } else {
        None
    };

    ScoredTerm {
        term: MatchCase {
            pattern: Some(pattern.term),
            source: Some(source.term),
            free_count: match_case.free_count,
            guard: guard_term,
        },
        score: Tree::Node(vec![
            pattern.score,
            source.score,
            Tree::<ScoreAtom>::create_leaf_from_i64(match_case.free_count as i64),
            guard_score,
        ]),
    }
}

pub fn combine_match(
    m: &Match,
    target: ScoredTerm<Par>,
    cases: Vec<ScoredTerm<MatchCase>>,
) -> ScoredTerm<Match> {
    let (case_terms, case_scores) = split_scored_terms(cases);

    ScoredTerm {
        term: Match {
            target: Some(target.term),
            cases: case_terms,
            locally_free: m.locally_free.clone(),
            connective_used: m.connective_used,
        },
        score: Tree::<ScoreAtom>::create_node_from_i32(
            Score::MATCH,
            std::iter::once(target.score)
                .chain(case_scores)
                .chain(std::iter::once(Tree::<ScoreAtom>::create_leaf_from_i64(
                    flag_score(m.connective_used),
                )))
                .collect(),
        ),
    }
}

pub fn combine_if(
    i: &If,
    condition: ScoredTerm<Par>,
    if_true: ScoredTerm<Par>,
    if_false: ScoredTerm<Par>,
) -> ScoredTerm<If> {
    ScoredTerm {
        term: If {
            condition: Some(condition.term),
            if_true: Some(if_true.term),
            if_false: Some(if_false.term),
            locally_free: i.locally_free.clone(),
            connective_used: i.connective_used,
        },
        score: Tree::<ScoreAtom>::create_node_from_i32(Score::IF, vec![
            condition.score,
            if_true.score,
            if_false.score,
            Tree::<ScoreAtom>::create_leaf_from_i64(flag_score(i.connective_used)),
        ]),
    }
}

pub fn combine_bundle(b: &Bundle, body: ScoredTerm<Par>) -> ScoredTerm<Bundle> {
    let score = if b.write_flag && b.read_flag {
        Score::BUNDLE_READ_WRITE
    } else if b.write_flag && !b.read_flag {
        Score::BUNDLE_WRITE
    } else if !b.write_flag && b.read_flag {
        Score::BUNDLE_READ
    } else {
        Score::BUNDLE_EQUIV
    };

    ScoredTerm {
        term: {
            let mut b_cloned = b.clone();
            b_cloned.body = Some(body.term);
            b_cloned
        },
        score: Tree::<ScoreAtom>::create_node_from_i32(score, vec![body.score]),
    }
}

pub fn combine_connective(c: &Connective, kids: Vec<ScoredTerm<Par>>) -> ScoredTerm<Connective> {
    match &c.connective_instance {
        Some(ConnectiveInstance::ConnAndBody(cb)) => {
            let (terms, scores) = split_scored_terms(kids);
            ScoredTerm {
                term: Connective {
                    connective_instance: Some(ConnectiveInstance::ConnAndBody({
                        let mut cb_cloned = cb.clone();
                        cb_cloned.ps = terms;
                        cb_cloned
                    })),
                },
                score: Tree::<ScoreAtom>::create_node_from_i32(Score::CONNECTIVE_AND, scores),
            }
        }

        Some(ConnectiveInstance::ConnOrBody(cb)) => {
            let (terms, scores) = split_scored_terms(kids);
            ScoredTerm {
                term: Connective {
                    connective_instance: Some(ConnectiveInstance::ConnOrBody({
                        let mut cb_cloned = cb.clone();
                        cb_cloned.ps = terms;
                        cb_cloned
                    })),
                },
                score: Tree::<ScoreAtom>::create_node_from_i32(Score::CONNECTIVE_OR, scores),
            }
        }

        Some(ConnectiveInstance::ConnNotBody(_)) => {
            let mut kids = kids;
            let scored_par = kids
                .pop()
                .expect("combine_connective: ConnNotBody has exactly one child");
            ScoredTerm {
                term: Connective {
                    connective_instance: Some(ConnectiveInstance::ConnNotBody(scored_par.term)),
                },
                score: Tree::<ScoreAtom>::create_node_from_i32(Score::CONNECTIVE_NOT, vec![
                    scored_par.score,
                ]),
            }
        }

        Some(ConnectiveInstance::VarRefBody(v)) => ScoredTerm {
            term: Connective {
                connective_instance: Some(ConnectiveInstance::VarRefBody(v.clone())),
            },
            score: Tree::<ScoreAtom>::create_node_from_i64s(vec![
                Score::CONNECTIVE_VARREF as i64,
                v.index as i64,
                v.depth as i64,
            ]),
        },

        Some(ConnectiveInstance::ConnBool(b)) => ScoredTerm {
            term: Connective {
                connective_instance: Some(ConnectiveInstance::ConnBool(*b)),
            },
            score: Tree::<ScoreAtom>::create_node_from_i64s(vec![
                Score::CONNECTIVE_BOOL as i64,
                flag_score(*b),
            ]),
        },

        Some(ConnectiveInstance::ConnInt(b)) => ScoredTerm {
            term: Connective {
                connective_instance: Some(ConnectiveInstance::ConnInt(*b)),
            },
            score: Tree::<ScoreAtom>::create_node_from_i64s(vec![
                Score::CONNECTIVE_INT as i64,
                flag_score(*b),
            ]),
        },

        Some(ConnectiveInstance::ConnString(b)) => ScoredTerm {
            term: Connective {
                connective_instance: Some(ConnectiveInstance::ConnString(*b)),
            },
            score: Tree::<ScoreAtom>::create_node_from_i64s(vec![
                Score::CONNECTIVE_STRING as i64,
                flag_score(*b),
            ]),
        },

        Some(ConnectiveInstance::ConnUri(b)) => ScoredTerm {
            term: Connective {
                connective_instance: Some(ConnectiveInstance::ConnUri(*b)),
            },
            score: Tree::<ScoreAtom>::create_node_from_i64s(vec![
                Score::CONNECTIVE_URI as i64,
                flag_score(*b),
            ]),
        },

        Some(ConnectiveInstance::ConnByteArray(b)) => ScoredTerm {
            term: Connective {
                connective_instance: Some(ConnectiveInstance::ConnByteArray(*b)),
            },
            score: Tree::<ScoreAtom>::create_node_from_i64s(vec![
                Score::CONNECTIVE_BYTEARRAY as i64,
                flag_score(*b),
            ]),
        },

        None => ScoredTerm {
            term: Connective::default(),
            score: Tree::<ScoreAtom>::create_leaf_from_i64(Score::ABSENT as i64),
        },
    }
}

#[inline]
fn construct_expr(expr_instance: ExprInstance, score: Tree<ScoreAtom>) -> ScoredTerm<Expr> {
    ScoredTerm {
        term: Expr {
            expr_instance: Some(expr_instance),
        },
        score,
    }
}

#[inline]
fn one(kids: Vec<ScoredTerm<Par>>) -> ScoredTerm<Par> {
    let mut kids = kids;
    debug_assert_eq!(kids.len(), 1, "combine_expr: expected exactly one child");
    kids.pop()
        .expect("combine_expr: a unary arm must have one child")
}

#[inline]
fn two(kids: Vec<ScoredTerm<Par>>) -> (ScoredTerm<Par>, ScoredTerm<Par>) {
    let mut kids = kids;
    debug_assert_eq!(kids.len(), 2, "combine_expr: expected exactly two children");
    let second = kids
        .pop()
        .expect("combine_expr: a binary arm must have two children");
    let first = kids
        .pop()
        .expect("combine_expr: a binary arm must have two children");
    (first, second)
}

macro_rules! unary_arm {
    ($name:ident, $tag:ident, $variant:ident, $ctor:ident) => {
        #[inline(never)]
        fn $name(kids: Vec<ScoredTerm<Par>>) -> ScoredTerm<Expr> {
            let ScoredTerm { term, score } = one(kids);
            construct_expr(
                ExprInstance::$variant($ctor { p: Some(term) }),
                Tree::<ScoreAtom>::create_node_from_i32(Score::$tag, vec![score]),
            )
        }
    };
}

macro_rules! binary_arm {
    ($name:ident, $tag:ident, $variant:ident, $ctor:ident) => {
        #[inline(never)]
        fn $name(kids: Vec<ScoredTerm<Par>>) -> ScoredTerm<Expr> {
            let (first, second) = two(kids);
            let ScoredTerm {
                term: t1,
                score: s1,
            } = first;
            let ScoredTerm {
                term: t2,
                score: s2,
            } = second;
            construct_expr(
                ExprInstance::$variant($ctor {
                    p1: Some(t1),
                    p2: Some(t2),
                }),
                Tree::<ScoreAtom>::create_node_from_i32(Score::$tag, vec![s1, s2]),
            )
        }
    };
}

unary_arm!(combine_eneg, ENEG, ENegBody, ENeg);
unary_arm!(combine_enot, ENOT, ENotBody, ENot);

binary_arm!(combine_emult, EMULT, EMultBody, EMult);
binary_arm!(combine_ediv, EDIV, EDivBody, EDiv);
binary_arm!(combine_emod, EMOD, EModBody, EMod);
binary_arm!(combine_eplus, EPLUS, EPlusBody, EPlus);
binary_arm!(combine_eminus, EMINUS, EMinusBody, EMinus);
binary_arm!(combine_elt, ELT, ELtBody, ELt);
binary_arm!(combine_elte, ELTE, ELteBody, ELte);
binary_arm!(combine_egt, EGT, EGtBody, EGt);
binary_arm!(combine_egte, EGTE, EGteBody, EGte);
binary_arm!(combine_eeq, EEQ, EEqBody, EEq);
binary_arm!(combine_eneq, ENEQ, ENeqBody, ENeq);
binary_arm!(combine_eand, EAND, EAndBody, EAnd);
binary_arm!(combine_eor, EOR, EOrBody, EOr);
binary_arm!(
    combine_epercentpercent,
    EPERCENT,
    EPercentPercentBody,
    EPercentPercent
);
binary_arm!(combine_eplusplus, EPLUSPLUS, EPlusPlusBody, EPlusPlus);
binary_arm!(
    combine_eminusminus,
    EMINUSMINUS,
    EMinusMinusBody,
    EMinusMinus
);

#[inline(never)]
fn combine_evar(ev: &EVar) -> ScoredTerm<Expr> {
    let sorted_var = sort_var(ev.v.as_ref().expect("var field was None, should be Some"));
    construct_expr(
        ExprInstance::EVarBody(EVar {
            v: Some(sorted_var.term),
        }),
        Tree::<ScoreAtom>::create_node_from_i32(Score::EVAR, vec![sorted_var.score]),
    )
}

#[inline(never)]
fn combine_ematches(kids: Vec<ScoredTerm<Par>>) -> ScoredTerm<Expr> {
    let (target, pattern) = two(kids);
    let ScoredTerm {
        term: target_term,
        score: target_score,
    } = target;
    let ScoredTerm {
        term: pattern_term,
        score: pattern_score,
    } = pattern;
    construct_expr(
        ExprInstance::EMatchesBody(EMatches {
            target: Some(target_term),
            pattern: Some(pattern_term),
        }),
        Tree::<ScoreAtom>::create_node_from_i32(Score::EMATCHES, vec![target_score, pattern_score]),
    )
}

#[inline(never)]
fn combine_elist(list: &EList, kids: Vec<ScoredTerm<Par>>) -> ScoredTerm<Expr> {
    let remainder_score = remainder_score(&list.remainder);
    let connective_used_score: i64 = flag_score(list.connective_used);
    let (element_terms, element_scores) = split_scored_terms(kids);

    construct_expr(
        ExprInstance::EListBody(EList {
            ps: element_terms,
            locally_free: list.locally_free.clone(),
            connective_used: list.connective_used,
            remainder: list.remainder.clone(),
        }),
        Tree::Node(
            vec![
                Tree::<ScoreAtom>::create_leaf_from_i64(Score::ELIST as i64),
                remainder_score,
            ]
            .into_iter()
            .chain(element_scores)
            .chain(std::iter::once(Tree::<ScoreAtom>::create_leaf_from_i64(
                connective_used_score,
            )))
            .collect(),
        ),
    )
}

#[inline(never)]
fn combine_etuple(tuple: &ETuple, kids: Vec<ScoredTerm<Par>>) -> ScoredTerm<Expr> {
    let connective_used_score: i64 = flag_score(tuple.connective_used);
    let (element_terms, element_scores) = split_scored_terms(kids);
    let tuple_cloned = ETuple {
        ps: element_terms,
        locally_free: tuple.locally_free.clone(),
        connective_used: tuple.connective_used,
    };

    construct_expr(
        ExprInstance::ETupleBody(tuple_cloned),
        Tree::Node(
            vec![Tree::<ScoreAtom>::create_leaf_from_i64(
                Score::ETUPLE as i64,
            )]
            .into_iter()
            .chain(element_scores)
            .chain(std::iter::once(Tree::<ScoreAtom>::create_leaf_from_i64(
                connective_used_score,
            )))
            .collect(),
        ),
    )
}

#[inline(never)]
fn combine_ezipper(zipper: &EZipper, kids: Vec<ScoredTerm<Par>>) -> ScoredTerm<Expr> {
    let pathmap = zipper.pathmap.as_ref().expect("zipper pathmap was None");
    let connective_used_score: i64 = flag_score(zipper.connective_used);
    let (element_terms, element_scores) = split_scored_terms(kids);

    construct_expr(
        ExprInstance::EZipperBody(EZipper {
            pathmap: Some(EPathMap {
                ps: element_terms,
                locally_free: pathmap.locally_free.clone(),
                connective_used: pathmap.connective_used,
                remainder: pathmap.remainder.clone(),
            }),
            current_path: zipper.current_path.clone(),
            is_write_zipper: zipper.is_write_zipper,
            locally_free: zipper.locally_free.clone(),
            connective_used: zipper.connective_used,
        }),
        Tree::Node(
            vec![Tree::<ScoreAtom>::create_leaf_from_i64(
                Score::EPATHMAP as i64 + 1,
            )]
            .into_iter()
            .chain(element_scores)
            .chain(std::iter::once(Tree::<ScoreAtom>::create_leaf_from_i64(
                connective_used_score,
            )))
            .collect(),
        ),
    )
}

#[inline(never)]
fn combine_emethod(em: &EMethod, kids: Vec<ScoredTerm<Par>>) -> ScoredTerm<Expr> {
    let mut kids = kids;
    let sorted_target = kids
        .pop()
        .expect("combine_expr: EMethodBody must have a target child");
    let connective_used_score: i64 = flag_score(em.connective_used);
    let (arg_terms, arg_scores) = split_scored_terms(kids);

    let mut em_cloned = em.clone();
    em_cloned.arguments = arg_terms;
    em_cloned.target = Some(sorted_target.term);

    construct_expr(
        ExprInstance::EMethodBody(em_cloned),
        Tree::Node(
            vec![
                Tree::<ScoreAtom>::create_leaf_from_i64(Score::EMETHOD as i64),
                Tree::<ScoreAtom>::create_leaf_from_string(em.method_name.clone()),
                sorted_target.score,
            ]
            .into_iter()
            .chain(arg_scores)
            .chain(std::iter::once(Tree::<ScoreAtom>::create_leaf_from_i64(
                connective_used_score,
            )))
            .collect(),
        ),
    )
}

#[inline(never)]
fn combine_gbool(e: &Expr, gb: bool) -> ScoredTerm<Expr> {
    let score = if gb {
        Tree::<ScoreAtom>::create_node_from_i64s(vec![Score::BOOL as i64, 0])
    } else {
        Tree::<ScoreAtom>::create_node_from_i64s(vec![Score::BOOL as i64, 1])
    };
    ScoredTerm {
        term: e.clone(),
        score,
    }
}

#[inline(never)]
fn combine_gint(e: &Expr, gi: i64) -> ScoredTerm<Expr> {
    ScoredTerm {
        term: e.clone(),
        score: Tree::<ScoreAtom>::create_node_from_i64s(vec![Score::INT as i64, gi]),
    }
}

#[inline(never)]
fn combine_gstring(e: &Expr, gs: &str) -> ScoredTerm<Expr> {
    ScoredTerm {
        term: e.clone(),
        score: Tree::<ScoreAtom>::create_node_from_i32(Score::STRING, vec![
            Tree::<ScoreAtom>::create_leaf_from_string(gs.to_string()),
        ]),
    }
}

#[inline(never)]
fn combine_guri(e: &Expr, gu: &str) -> ScoredTerm<Expr> {
    ScoredTerm {
        term: e.clone(),
        score: Tree::<ScoreAtom>::create_node_from_i32(Score::URI, vec![
            Tree::<ScoreAtom>::create_leaf_from_string(gu.to_string()),
        ]),
    }
}

#[inline(never)]
fn combine_gbytearray(e: &Expr, ba: &[u8]) -> ScoredTerm<Expr> {
    ScoredTerm {
        term: e.clone(),
        score: Tree::<ScoreAtom>::create_node_from_i32(Score::EBYTEARR, vec![
            Tree::<ScoreAtom>::create_leaf_from_bytes(ba.to_vec()),
        ]),
    }
}

#[inline(never)]
fn combine_gdouble(e: &Expr, bits: u64) -> ScoredTerm<Expr> {
    ScoredTerm {
        term: e.clone(),
        score: Tree::<ScoreAtom>::create_node_from_i64s(vec![Score::DOUBLE as i64, bits as i64]),
    }
}

#[inline(never)]
fn combine_gbigint(e: &Expr, bytes: &[u8]) -> ScoredTerm<Expr> {
    ScoredTerm {
        term: e.clone(),
        score: Tree::<ScoreAtom>::create_node_from_i32(Score::BIG_INT, vec![
            Tree::<ScoreAtom>::create_leaf_from_bytes(bytes.to_vec()),
        ]),
    }
}

#[inline(never)]
fn combine_gbigrat(e: &Expr, rat: &GBigRational) -> ScoredTerm<Expr> {
    ScoredTerm {
        term: e.clone(),
        score: Tree::<ScoreAtom>::create_node_from_i32(Score::BIG_RAT, vec![
            Tree::<ScoreAtom>::create_leaf_from_bytes(rat.numerator.clone()),
            Tree::<ScoreAtom>::create_leaf_from_bytes(rat.denominator.clone()),
        ]),
    }
}

#[inline(never)]
fn combine_gfixedpoint(e: &Expr, fp: &GFixedPoint) -> ScoredTerm<Expr> {
    ScoredTerm {
        term: e.clone(),
        score: Tree::<ScoreAtom>::create_node_from_i32(Score::FIXED_POINT, vec![
            Tree::<ScoreAtom>::create_leaf_from_bytes(fp.unscaled.clone()),
            Tree::<ScoreAtom>::create_node_from_i64s(vec![fp.scale as i64]),
        ]),
    }
}

#[inline(never)]
fn combine_expr_absent(e: &Expr) -> ScoredTerm<Expr> {
    ScoredTerm {
        term: e.clone(),
        score: Tree::<ScoreAtom>::create_node_from_i32(Score::ABSENT, Vec::new()),
    }
}

pub fn combine_expr(e: &Expr, kids: Vec<ScoredTerm<Par>>) -> ScoredTerm<Expr> {
    match &e.expr_instance {
        Some(expr) => match expr {
            ExprInstance::ENegBody(_) => combine_eneg(kids),
            ExprInstance::ENotBody(_) => combine_enot(kids),
            ExprInstance::EVarBody(ev) => combine_evar(ev),

            ExprInstance::EMultBody(_) => combine_emult(kids),
            ExprInstance::EDivBody(_) => combine_ediv(kids),
            ExprInstance::EModBody(_) => combine_emod(kids),
            ExprInstance::EPlusBody(_) => combine_eplus(kids),
            ExprInstance::EMinusBody(_) => combine_eminus(kids),
            ExprInstance::ELtBody(_) => combine_elt(kids),
            ExprInstance::ELteBody(_) => combine_elte(kids),
            ExprInstance::EGtBody(_) => combine_egt(kids),
            ExprInstance::EGteBody(_) => combine_egte(kids),
            ExprInstance::EEqBody(_) => combine_eeq(kids),
            ExprInstance::ENeqBody(_) => combine_eneq(kids),
            ExprInstance::EAndBody(_) => combine_eand(kids),
            ExprInstance::EOrBody(_) => combine_eor(kids),
            ExprInstance::EMatchesBody(_) => combine_ematches(kids),
            ExprInstance::EPercentPercentBody(_) => combine_epercentpercent(kids),
            ExprInstance::EPlusPlusBody(_) => combine_eplusplus(kids),
            ExprInstance::EMinusMinusBody(_) => combine_eminusminus(kids),

            ExprInstance::EListBody(list) => combine_elist(list, kids),
            ExprInstance::ETupleBody(tuple) => combine_etuple(tuple, kids),
            ExprInstance::EZipperBody(zipper) => combine_ezipper(zipper, kids),
            ExprInstance::EMethodBody(em) => combine_emethod(em, kids),

            ExprInstance::EMapBody(emap) => combine_emap(emap),
            ExprInstance::ESetBody(eset) => combine_eset(eset),
            ExprInstance::EPathmapBody(pathmap) => combine_epathmap(pathmap, kids),

            ExprInstance::GBool(gb) => combine_gbool(e, *gb),
            ExprInstance::GInt(gi) => combine_gint(e, *gi),
            ExprInstance::GString(gs) => combine_gstring(e, gs),
            ExprInstance::GUri(gu) => combine_guri(e, gu),
            ExprInstance::GByteArray(ba) => combine_gbytearray(e, ba),
            ExprInstance::GDouble(bits) => combine_gdouble(e, *bits),
            ExprInstance::GFloat32(bits) => ScoredTerm {
                term: e.clone(),
                score: Tree::<ScoreAtom>::create_node_from_i64s(vec![
                    Score::FLOAT32 as i64,
                    *bits as i64,
                ]),
            },
            ExprInstance::GBigInt(bytes) => combine_gbigint(e, bytes),
            ExprInstance::GBigRat(rat) => combine_gbigrat(e, rat),
            ExprInstance::GFixedPoint(fp) => combine_gfixedpoint(e, fp),
            ExprInstance::GUint64(u) => ScoredTerm {
                term: e.clone(),
                score: Tree::<ScoreAtom>::create_node_from_i32(Score::UINT64, vec![
                    Tree::<ScoreAtom>::create_leaf_from_bytes(u.to_be_bytes().to_vec()),
                ]),
            },

            ExprInstance::GInt32(x) => ScoredTerm {
                term: e.clone(),
                score: Tree::<ScoreAtom>::create_node_from_i64s(vec![
                    Score::INT32 as i64,
                    *x as i64,
                ]),
            },

            ExprInstance::GUint32(x) => ScoredTerm {
                term: e.clone(),
                score: Tree::<ScoreAtom>::create_node_from_i64s(vec![
                    Score::UINT32 as i64,
                    *x as i64,
                ]),
            },

            ExprInstance::GUint16(x) => ScoredTerm {
                term: e.clone(),
                score: Tree::<ScoreAtom>::create_node_from_i64s(vec![
                    Score::UINT16 as i64,
                    *x as i64,
                ]),
            },

            ExprInstance::GUint8(x) => ScoredTerm {
                term: e.clone(),
                score: Tree::<ScoreAtom>::create_node_from_i64s(vec![
                    Score::UINT8 as i64,
                    *x as i64,
                ]),
            },
        },
        None => combine_expr_absent(e),
    }
}

#[inline(never)]
fn combine_emap(emap: &crate::rhoapi::EMap) -> ScoredTerm<Expr> {
    let mut position: HashMap<Par, usize> = HashMap::with_capacity(emap.kvs.len());
    let mut sorted_pars: Vec<ScoredTerm<(Par, Par)>> = Vec::with_capacity(emap.kvs.len());
    for kv in &emap.kvs {
        let scored = Ordering::sort_key_value_pair(
            kv.key.as_ref().expect("key field on KeyValuePair was None"),
            kv.value
                .as_ref()
                .expect("value field on KeyValuePair was None"),
        );
        match position.get(&scored.term.0) {
            Some(&at) => sorted_pars[at] = scored,
            None => {
                position.insert(scored.term.0.clone(), sorted_pars.len());
                sorted_pars.push(scored);
            }
        }
    }
    ScoredTerm::sort_vec(&mut sorted_pars);

    let remainder_score = remainder_score(&emap.remainder);
    let connective_used_score: i64 = if emap.connective_used { 1 } else { 0 };
    let (kvs, pair_scores): (Vec<KeyValuePair>, Vec<Tree<ScoreAtom>>) = sorted_pars
        .into_iter()
        .map(|p| {
            let (key, value) = p.term;
            (
                KeyValuePair {
                    key: Some(key),
                    value: Some(value),
                },
                p.score,
            )
        })
        .unzip();

    construct_expr(
        ExprInstance::EMapBody(EMap {
            kvs,
            locally_free: emap.locally_free.clone(),
            connective_used: emap.connective_used,
            remainder: emap.remainder.clone(),
        }),
        Tree::Node(
            vec![
                Tree::<ScoreAtom>::create_leaf_from_i64(Score::EMAP as i64),
                remainder_score,
            ]
            .into_iter()
            .chain(pair_scores)
            .chain(vec![Tree::<ScoreAtom>::create_leaf_from_i64(
                connective_used_score,
            )])
            .collect(),
        ),
    )
}

#[inline(never)]
fn combine_eset(eset: &crate::rhoapi::ESet) -> ScoredTerm<Expr> {
    let mut seen: HashSet<Par> = HashSet::with_capacity(eset.ps.len());
    let mut sorted_pars: Vec<ScoredTerm<Par>> = Vec::with_capacity(eset.ps.len());
    for p in &eset.ps {
        let scored = ParSortMatcher::sort_match(p);
        if seen.insert(scored.term.clone()) {
            sorted_pars.push(scored);
        }
    }
    ScoredTerm::sort_vec(&mut sorted_pars);

    let remainder_score = remainder_score(&eset.remainder);
    let connective_used_score: i64 = if eset.connective_used { 1 } else { 0 };
    let (ps, element_scores): (Vec<Par>, Vec<Tree<ScoreAtom>>) =
        sorted_pars.into_iter().map(|p| (p.term, p.score)).unzip();

    construct_expr(
        ExprInstance::ESetBody(ESet {
            ps,
            locally_free: eset.locally_free.clone(),
            connective_used: eset.connective_used,
            remainder: eset.remainder.clone(),
        }),
        Tree::Node(
            vec![
                Tree::<ScoreAtom>::create_leaf_from_i64(Score::ESET as i64),
                remainder_score,
            ]
            .into_iter()
            .chain(element_scores)
            .chain(vec![Tree::<ScoreAtom>::create_leaf_from_i64(
                connective_used_score,
            )])
            .collect(),
        ),
    )
}

#[inline(never)]
fn combine_epathmap(pathmap: &EPathMap, kids: Vec<ScoredTerm<Par>>) -> ScoredTerm<Expr> {
    let (terms, scores) = split_scored_terms(kids);
    construct_expr(
        ExprInstance::EPathmapBody(EPathMap {
            ps: terms,
            locally_free: pathmap.locally_free.clone(),
            connective_used: pathmap.connective_used,
            remainder: pathmap.remainder.clone(),
        }),
        Tree::Node(
            vec![
                Tree::<ScoreAtom>::create_leaf_from_i64(Score::EPATHMAP as i64),
                remainder_score(&pathmap.remainder),
            ]
            .into_iter()
            .chain(scores)
            .chain(std::iter::once(Tree::<ScoreAtom>::create_leaf_from_i64(
                flag_score(pathmap.connective_used),
            )))
            .collect(),
        ),
    )
}
