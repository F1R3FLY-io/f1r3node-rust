//! # The sorter's per-arm table — single-sourced
//!
//! Leg-2 Stage C-2 replaces the sorter's mutual recursion with an explicit heap
//! worklist ([`super::sort_drive`]). The conversion is checked against a
//! recursive oracle ([`super::sort_recursive`]), and the two would be worthless
//! as a differential if each carried its own copy of the 36-arm `ExprInstance`
//! table: they would drift, and a divergence would be reported as a *traversal*
//! bug when it was an *arm* bug.
//!
//! So this module owns both halves of every node's handling, and the two
//! traversals differ **only** in shape:
//!
//! | half | what it answers | used by |
//! |---|---|---|
//! | `*_child_pars` / the `descend_*` slot lists | *which* sub-terms this node contains, and in what order | the driver's descent **and** the oracle's recursion |
//! | `combine_*` | how the node is rebuilt from its children's [`ScoredTerm`]s | the driver's `Combine` **and** the oracle's post-order |
//!
//! ```text
//!                    ┌──────────────── sort_combine (this module) ────────────────┐
//!                    │   child slots  ·  combine_par / combine_expr / combine_…   │
//!                    └───────▲────────────────────────────────────▲───────────────┘
//!                            │                                    │
//!              ┌─────────────┴─────────────┐        ┌─────────────┴─────────────┐
//!              │  sort_drive (production)  │        │ sort_recursive (#[cfg(test)]) │
//!              │  explicit worklist, O(1)  │        │  the recursive ORACLE      │
//!              └───────────────────────────┘        └───────────────────────────┘
//! ```
//!
//! ## ★ Why the obligation here is stricter than "no panic"
//!
//! `rholang/src/rust/interpreter/accounting/cost_accounting/sig.rs` computes
//! `ParSortMatcher::sort_match(&par).term.encode_to_vec()` — **the bytes that
//! get signed**. A one-element reordering is a consensus fork. Every function
//! below is therefore a *verbatim* transcription of the corresponding arm of
//! the pre-conversion matcher, and three independent checks stand behind that
//! claim:
//!
//! 1. `models/tests/sorter_canonical_golden.rs` pins the encoded term **and**
//!    the score of a corpus covering every `ExprInstance` variant against a
//!    fixture captured from the pre-conversion implementation. This is the
//!    check the shared table cannot fake: a transcription error moves the
//!    golden, whichever traversal produced it.
//! 2. `super::sort_recursive`'s differential compares the two traversal shapes
//!    over the same corpus, including multi-sibling terms where a reversed
//!    `Combine` would otherwise be invisible.
//! 3. `rholang/tests/stack_depth_gate.rs` proves the driver is the one with
//!    `O(1)` native stack.
//!
//! ## ⚠ Three arms are deliberately NOT worklisted
//!
//! `ESetBody`, `EMapBody` and `EPathmapBody` route their elements through
//! `SortedParHashSet` / `SortedParMap` / `canonicalize_ground_epathmap`, which
//! **re-enter** `ParSortMatcher::sort_match` on *owned intermediates* (the
//! deduplicated set, the canonicalised trie order) rather than on sub-terms of
//! the input. Those intermediates cannot be borrowed from the root, so they
//! cannot become worklist children without either an arena or an extra deep
//! copy of the whole term.
//!
//! They are therefore kept **verbatim**, and each re-entry is its own bounded
//! drive — the discipline `reduce.rs` already uses for owned intermediates. Two
//! consequences, both measured rather than assumed and both named in the gate:
//!
//! * a chain of `n` nested sets/maps costs `n` *drive frames* rather than `n`
//!   *sorter frames* — the subject `sort_nested_set` in
//!   `rholang/tests/stack_depth_gate.rs`;
//! * `HashSet<Par>` / `HashMap<Par, Par>` operations invoke the **derived**
//!   `Par: Clone + Hash + Eq`, each of which is itself Θ(depth)
//!   (`<Par as Clone>::clone` measured at 15,875 B/level, debug). Those are row
//!   5 of the audit's table, whose disposition is explicitly "Leg-1 only:
//!   remove the call sites, not the impl", so no conversion of the *sorter*
//!   can remove them. Keeping the arms verbatim keeps that residual exactly
//!   where it already was instead of relocating it.
//!
//! ⚠ It also preserves something subtler that a restructuring would silently
//! break: `HashSet`'s iteration order comes from a `RandomState` seeded per
//! instance from a thread-local counter, so the *number and order* of
//! `HashSet` constructions is observable whenever two distinct elements have
//! equal scores (the sorter is a normalizer, hence not injective). Verbatim
//! arms construct exactly the same sets in exactly the same order.
//!
//! Full analysis, measured constants and proof standard:
//! `docs/design/audits/theta-depth-traversals-2026-07-26.md`.

use std::sync::OnceLock;

use super::score_tree::{Score, ScoreAtom, ScoredTerm, Tree};
use crate::rhoapi::connective::ConnectiveInstance;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::g_unforgeable::UnfInstance;
use crate::rhoapi::var::VarInstance;
use crate::rhoapi::{
    Bundle, Connective, EAnd, EDiv, EEq, EGt, EGte, EList, ELt, ELte, EMatches, EMinus,
    EMinusMinus, EMod, EMult, ENeg, ENeq, ENot, EOr, EPathMap, EPercentPercent, EPlus, EPlusPlus,
    EMethod, ETuple, EVar, EZipper, Expr, GBigRational, GFixedPoint, GUnforgeable, If, Match,
    MatchCase, New, Par, Receive, ReceiveBind, Send, Var,
};
use crate::rust::par_map::ParMap;
use crate::rust::par_map_type_mapper::ParMapTypeMapper;
use crate::rust::par_set::ParSet;
use crate::rust::par_set_type_mapper::ParSetTypeMapper;
use crate::rust::pathmap_crate_type_mapper::eval_stable_epathmap;
use crate::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use crate::rust::rholang::sorter::sortable::Sortable;
use crate::rust::sorted_par_hash_set::SortedParHashSet;

// ===========================================================================
// small shared helpers
// ===========================================================================

/// Split a `Vec<ScoredTerm<T>>` into its terms and its scores **by move**.
///
/// The pre-conversion matchers wrote `xs.clone().into_iter().map(|x| x.term)`
/// followed by `xs.into_iter().map(|x| x.score)`, which deep-copied every term
/// *and* every score tree once per node — a Θ(depth) `<Par as Clone>::clone`
/// per sibling, on the reduce path. This is the Leg-1 de-cloning of the sorter:
/// same values, no copy.
pub fn split_scored_terms<T>(scored_terms: Vec<ScoredTerm<T>>) -> (Vec<T>, Vec<Tree<ScoreAtom>>) {
    let mut terms = Vec::with_capacity(scored_terms.len());
    let mut scores = Vec::with_capacity(scored_terms.len());

    for scored in scored_terms {
        terms.push(scored.term);
        scores.push(scored.score);
    }

    (terms, scores)
}

/// The `Par` that a missing optional stands in for.
///
/// The pre-conversion `ReceiveSortMatcher` / `MatchSortMatcher` wrote
/// `r.condition.clone().unwrap_or_default()` and sorted *that*. Cloning a
/// present condition is a Θ(depth) `<Par as Clone>::clone` for a value the
/// sorter only reads, so the driver sorts `r.condition.as_ref()` directly and
/// falls back to this shared empty `Par` when the field is absent.
/// `sort_match(&Par::default())` is a pure function of its argument, so the two
/// spellings are the same value.
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

// ===========================================================================
// LEAVES — nodes with no child `Par`, scored in place
// ===========================================================================

/// `VarSortMatcher::sort_match`, verbatim. A `Var` has no sub-`Par`, so it is a
/// leaf for every traversal in this family.
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

/// `UnforgeableSortMatcher::sort_match`, verbatim. Also a leaf: every arm
/// carries bytes, never a `Par`.
pub fn sort_unforgeable(unf: &GUnforgeable) -> ScoredTerm<GUnforgeable> {
    match &unf.unf_instance {
        Some(inner) => match inner {
            UnfInstance::GPrivateBody(gpriv) => ScoredTerm {
                term: GUnforgeable {
                    unf_instance: Some(UnfInstance::GPrivateBody(gpriv.clone())),
                },
                score: Tree::<ScoreAtom>::create_node_from_i32(
                    Score::PRIVATE,
                    vec![Tree::<ScoreAtom>::create_leaf_from_bytes(gpriv.id.clone())],
                ),
            },

            UnfInstance::GDeployerIdBody(id) => ScoredTerm {
                term: GUnforgeable {
                    unf_instance: Some(UnfInstance::GDeployerIdBody(id.clone())),
                },
                score: Tree::<ScoreAtom>::create_node_from_i32(
                    Score::DEPLOYER_AUTH,
                    vec![Tree::<ScoreAtom>::create_leaf_from_bytes(
                        id.public_key.clone(),
                    )],
                ),
            },

            UnfInstance::GDeployIdBody(deploy_id) => ScoredTerm {
                term: GUnforgeable {
                    unf_instance: Some(UnfInstance::GDeployIdBody(deploy_id.clone())),
                },
                score: Tree::<ScoreAtom>::create_node_from_i32(
                    Score::DEPLOY_ID,
                    vec![Tree::<ScoreAtom>::create_leaf_from_bytes(
                        deploy_id.sig.clone(),
                    )],
                ),
            },

            UnfInstance::GSysAuthTokenBody(token) => ScoredTerm {
                term: GUnforgeable {
                    unf_instance: Some(UnfInstance::GSysAuthTokenBody(token.clone())),
                },
                score: Tree::<ScoreAtom>::create_node_from_i32(Score::SYS_AUTH_TOKEN, vec![]),
            },
        },
        None => ScoredTerm {
            term: unf.clone(),
            score: Tree::<ScoreAtom>::create_node_from_i32(Score::ABSENT, Vec::new()),
        },
    }
}

// ===========================================================================
// CHILD SLOTS — the one place that answers "which sub-`Par`s, in what order?"
//
// Both traversals call these. The order is the order the pre-conversion
// matcher evaluated its recursive calls in, which is what makes an `expect`
// on a missing required field fire at the same point on both paths.
// ===========================================================================

/// The sub-`Par`s of an `Expr`, in evaluation order.
///
/// ⚠ Returns **nothing** for `ESetBody`, `EMapBody` and `EPathmapBody`: those
/// arms are self-contained (module docs). It also returns nothing for
/// `EVarBody`, whose child is a `Var` — a leaf — scored inside
/// [`combine_expr`].
pub fn expr_child_pars<'t>(e: &'t Expr, out: &mut Vec<&'t Par>) {
    let Some(instance) = e.expr_instance.as_ref() else {
        return;
    };
    match instance {
        // ---- grounds and the variable: no child `Par` ----
        ExprInstance::GBool(_)
        | ExprInstance::GInt(_)
        | ExprInstance::GString(_)
        | ExprInstance::GUri(_)
        | ExprInstance::GByteArray(_)
        | ExprInstance::GDouble(_)
        | ExprInstance::GBigInt(_)
        | ExprInstance::GBigRat(_)
        | ExprInstance::GFixedPoint(_)
        | ExprInstance::EVarBody(_) => {}

        // ---- self-contained arms; see the module docs ----
        ExprInstance::ESetBody(_)
        | ExprInstance::EMapBody(_)
        | ExprInstance::EPathmapBody(_) => {}

        // ---- unary ----
        ExprInstance::ENegBody(x) => out.push(required(&x.p)),
        ExprInstance::ENotBody(x) => out.push(required(&x.p)),

        // ---- binary: p1 then p2 ----
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

        // ---- n-ary, order-preserving ----
        ExprInstance::EListBody(x) => out.extend(x.ps.iter()),
        ExprInstance::ETupleBody(x) => out.extend(x.ps.iter()),
        ExprInstance::EZipperBody(x) => out.extend(
            x.pathmap
                .as_ref()
                .expect("zipper pathmap was None")
                .ps()
                .iter(),
        ),

        // ---- ⚠ arguments BEFORE target: that is the pre-conversion order ----
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

/// The `.expect` message the pre-conversion arms used for every required
/// `Option<Par>` slot on an arithmetic / relational / logical `Expr`.
fn required(slot: &Option<Par>) -> &Par {
    slot.as_ref().expect("par field was None, should be Some")
}

/// The sub-`Par`s of a `Connective`, in evaluation order.
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

// ===========================================================================
// COMBINE — how each node is rebuilt from its children's `ScoredTerm`s
// ===========================================================================

/// The nine category vectors of a `Par`, already sorted individually.
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
}

/// `ParSortMatcher::sort_match`'s post-order half.
///
/// ⚠ Each category is ordered by [`ScoredTerm::sort_vec`] — a **stable** sort
/// (`sort_by`, never `sort_unstable_by`). The comparator returns `Equal` for
/// distinct terms with equal scores, so an unstable sort would be free to
/// reorder them and fork the canonical form. See `score_tree.rs`.
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

    let (send_terms, send_scores) = split_scored_terms(sends);
    let (receive_terms, receive_scores) = split_scored_terms(receives);
    let (news_terms, news_scores) = split_scored_terms(news);
    let (expr_terms, expr_scores) = split_scored_terms(exprs);
    let (match_terms, match_scores) = split_scored_terms(matches);
    let (bundle_terms, bundle_scores) = split_scored_terms(bundles);
    let (connective_terms, connective_scores) = split_scored_terms(connectives);
    let (unforgeable_terms, unforgeable_scores) = split_scored_terms(unforgeables);
    let (conditional_terms, conditional_scores) = split_scored_terms(conditionals);

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
        locally_free: par.locally_free.clone(),
        connective_used: par.connective_used,
    };

    // ⚠ The score's child order is NOT the term's field order: it is
    // send, receive, EXPR, news, match, bundle, connective, unforgeable,
    // conditional — exactly as the pre-conversion matcher chained them.
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

/// `SendSortMatcher::sort_match`'s post-order half. Children: `chan`, then
/// `data` in order.
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

/// `ReceiveSortMatcher::sort_bind`'s post-order half. Children: `patterns` in
/// order, then `source`.
///
/// ⚠ The rebuilt `ReceiveBind` keeps the **original** `remainder`, while the
/// score uses the *sorted* remainder — that asymmetry is in the pre-conversion
/// matcher and is reproduced verbatim, because the term is what gets signed.
pub fn combine_bind(
    bind: &ReceiveBind,
    patterns: Vec<ScoredTerm<Par>>,
    channel: ScoredTerm<Par>,
) -> ScoredTerm<ReceiveBind> {
    let sorted_remainder_score = match &bind.remainder {
        Some(bind_remainder) => sort_var(bind_remainder).score,
        None => Tree::<ScoreAtom>::create_leaf_from_i64(Score::ABSENT as i64),
    };

    let (pattern_terms, pattern_scores) = split_scored_terms(patterns);

    ScoredTerm {
        term: ReceiveBind {
            patterns: pattern_terms,
            source: Some(channel.term),
            remainder: bind.remainder.clone(),
            free_count: bind.free_count,
        },
        score: Tree::Node(
            std::iter::once(channel.score)
                .chain(pattern_scores)
                .chain(std::iter::once(sorted_remainder_score))
                .collect(),
        ),
    }
}

/// `ReceiveSortMatcher::sort_match`'s post-order half. Children: `binds` in
/// order, then `body`, then the (possibly absent) `condition`.
///
/// ⚠ `Some(empty Par)` collapses to `None` on the output term — `eval_receive`
/// treats an empty condition as absent, so preserving the distinction on the
/// wire would leak a difference the runtime does not have. The *score* always
/// includes the sorted condition, so an un-guarded receive still hashes
/// deterministically. Both behaviours are the pre-conversion matcher's.
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

/// `NewSortMatcher::sort_match`'s post-order half. Children: `p`, then the
/// `injections` values in `BTreeMap` (i.e. key) order.
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
        vec![Tree::<ScoreAtom>::create_leaf_from_i64(Score::ABSENT as i64)]
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
        vec![Tree::<ScoreAtom>::create_leaf_from_i64(Score::ABSENT as i64)]
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

/// `MatchSortMatcher::sort_case`'s post-order half. Children: `pattern`,
/// `source`, then the (possibly absent) `guard`.
///
/// ⚠ Same `Some(empty Par)` → `None` collapse as [`combine_receive`], for the
/// same reason: the four guard sites already treat an empty guard as absent.
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
    let guard_term = if guard_present { Some(guard_term) } else { None };

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

/// `MatchSortMatcher::sort_match`'s post-order half. Children: `target`, then
/// `cases` in order.
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

/// `IfSortMatcher::sort_match`'s post-order half. Children: `condition`,
/// `if_true`, `if_false`.
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
        score: Tree::<ScoreAtom>::create_node_from_i32(
            Score::IF,
            vec![
                condition.score,
                if_true.score,
                if_false.score,
                Tree::<ScoreAtom>::create_leaf_from_i64(flag_score(i.connective_used)),
            ],
        ),
    }
}

/// `BundleSortMatcher::sort_match`'s post-order half. One child: `body`.
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

/// `ConnectiveSortMatcher::sort_match`'s post-order half. Children are those
/// listed by [`connective_child_pars`].
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
                score: Tree::<ScoreAtom>::create_node_from_i32(
                    Score::CONNECTIVE_NOT,
                    vec![scored_par.score],
                ),
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

// ===========================================================================
// `Expr` — the DISPATCHER and its thirty-six per-arm functions
// ===========================================================================
//
// ## ★ Why every arm is its own `#[inline(never)]` function
//
// At `-O0` rustc does **not** overlap the stack slots of mutually exclusive
// `match` arms. A single function carrying all 36 `ExprInstance` arms' locals
// therefore costs a frame the size of their *sum*: measured by `gdb` on the
// draft of this conversion, `combine_expr` was **99,888 bytes** — larger than
// the whole recursive `ExprSortMatcher::sort_match` it replaced (69,744 B).
//
// That is harmless on the ordinary spine, where the driver is flat and the
// frame is entered once (measured: **0 B/level**, parameter 4 → 4,096, both
// profiles). It is *not* harmless on the three self-contained arms
// (`ESetBody`, `EMapBody`, `EPathmapBody`), which re-enter
// `ParSortMatcher::sort_match` on owned intermediates and therefore keep one
// `combine_expr` frame alive **per nesting level**.
//
// ⚠ Hoisting only those three arms out of line was measured and **refuted**:
// 130,458 → 127,4xx B/level, a 2.6 % move, because the other 33 arms' locals
// still held the frame. The refutation refutes hoisting *three of thirty-six*,
// not hoisting — so the split is applied to **every** arm. Each arm's frame is
// then bounded by its own locals **by construction**, which is a structural
// statement rather than a measured one, and the split is pure code motion, so
// the canonical form cannot move.
//
// Two alternatives were considered and rejected:
//
// * *Boxed / uniform arm locals* — would change how many heap containers each
//   arm builds and in what order, and `HashSet`'s iteration order is seeded
//   from a per-thread counter, so container construction order is
//   consensus-observable whenever two distinct elements share a score.
// * *Eliminating the re-entrancy* — a large change for a member the `3^n`
//   bound (below) makes unreachable in practice.

/// The `ScoredTerm<Expr>` every arm builds.
#[inline]
fn construct_expr(expr_instance: ExprInstance, score: Tree<ScoreAtom>) -> ScoredTerm<Expr> {
    ScoredTerm {
        term: Expr {
            expr_instance: Some(expr_instance),
        },
        score,
    }
}

/// A one-child arm: take the single scored child.
#[inline]
fn one(kids: Vec<ScoredTerm<Par>>) -> ScoredTerm<Par> {
    let mut kids = kids;
    debug_assert_eq!(kids.len(), 1, "combine_expr: expected exactly one child");
    kids.pop()
        .expect("combine_expr: a unary arm must have one child")
}

/// A two-child arm: take them in slot order.
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

/// `-p` / `~p`: one child `Par`, scored `node_i32(tag, [child])`.
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

/// `p1 OP p2`: two child `Par`s in slot order, scored `node_i32(tag, [s1, s2])`.
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

/// `EVarBody`. Its child is a `Var` — a leaf — so it takes no worklist
/// children and scores in place.
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

/// `EMatchesBody`. Slot order is `target`, then `pattern`.
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
        Tree::<ScoreAtom>::create_node_from_i32(
            Score::EMATCHES,
            vec![target_score, pattern_score],
        ),
    )
}

/// `EListBody`. Order-preserving: the emitted `ps` is the input order.
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

/// `ETupleBody`. ⚠ No remainder slot, unlike `EListBody`.
#[inline(never)]
fn combine_etuple(tuple: &ETuple, kids: Vec<ScoredTerm<Par>>) -> ScoredTerm<Expr> {
    let connective_used_score: i64 = flag_score(tuple.connective_used);
    let (element_terms, element_scores) = split_scored_terms(kids);
    let mut tuple_cloned = tuple.clone();
    tuple_cloned.ps = element_terms;

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

/// `EZipperBody`. Scores under `EPATHMAP + 1`; no remainder slot.
#[inline(never)]
fn combine_ezipper(zipper: &EZipper, kids: Vec<ScoredTerm<Par>>) -> ScoredTerm<Expr> {
    let pathmap = zipper.pathmap.as_ref().expect("zipper pathmap was None");
    let connective_used_score: i64 = flag_score(zipper.connective_used);
    let (element_terms, element_scores) = split_scored_terms(kids);

    construct_expr(
        ExprInstance::EZipperBody(EZipper {
            // EPathMap fix P3 (PM-2): constructor instead of
            // a struct literal (private shadow cell).
            pathmap: Some(EPathMap::new(
                element_terms,
                pathmap.locally_free.clone(),
                pathmap.connective_used,
                pathmap.remainder.clone(),
            )),
            current_path: zipper.current_path.clone(),
            is_write_zipper: zipper.is_write_zipper,
            locally_free: zipper.locally_free.clone(),
            connective_used: zipper.connective_used,
            // The cursor's split/bare discriminator travels with
            // `current_path`: sorting canonicalizes the zipper's MAP, never
            // its focus.
            cursor_kind: zipper.cursor_kind,
        }),
        Tree::Node(
            vec![
                // Use EPATHMAP + 1 for zipper
                Tree::<ScoreAtom>::create_leaf_from_i64(Score::EPATHMAP as i64 + 1),
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

/// `EMethodBody`.
///
/// ⚠ `expr_child_pars` pushes the ARGUMENTS first and the TARGET last, matching
/// the pre-conversion evaluation order, while the *score* lists the target
/// before the arguments. Both asymmetries are reproduced verbatim.
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

// ---- grounds: the term is `e.clone()` (the whole `Expr`), never rebuilt ----

/// ⚠ `GBool` scores `[BOOL, 0]` when the value is **true** and `[BOOL, 1]` when
/// it is false. That inversion is in the pre-conversion matcher (and in
/// `BoolSortMatcher.scala`) and is reproduced verbatim: it is part of the
/// canonical form, and the canonical form is signed.
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
        score: Tree::<ScoreAtom>::create_node_from_i32(
            Score::STRING,
            vec![Tree::<ScoreAtom>::create_leaf_from_string(gs.to_string())],
        ),
    }
}

#[inline(never)]
fn combine_guri(e: &Expr, gu: &str) -> ScoredTerm<Expr> {
    ScoredTerm {
        term: e.clone(),
        score: Tree::<ScoreAtom>::create_node_from_i32(
            Score::URI,
            vec![Tree::<ScoreAtom>::create_leaf_from_string(gu.to_string())],
        ),
    }
}

#[inline(never)]
fn combine_gbytearray(e: &Expr, ba: &[u8]) -> ScoredTerm<Expr> {
    ScoredTerm {
        term: e.clone(),
        score: Tree::<ScoreAtom>::create_node_from_i32(
            Score::EBYTEARR,
            vec![Tree::<ScoreAtom>::create_leaf_from_bytes(ba.to_vec())],
        ),
    }
}

#[inline(never)]
fn combine_gdouble(e: &Expr, bits: u64) -> ScoredTerm<Expr> {
    ScoredTerm {
        term: e.clone(),
        score: Tree::<ScoreAtom>::create_node_from_i64s(vec![
            Score::DOUBLE as i64,
            bits as i64,
        ]),
    }
}

#[inline(never)]
fn combine_gbigint(e: &Expr, bytes: &[u8]) -> ScoredTerm<Expr> {
    ScoredTerm {
        term: e.clone(),
        score: Tree::<ScoreAtom>::create_node_from_i32(
            Score::BIG_INT,
            vec![Tree::<ScoreAtom>::create_leaf_from_bytes(bytes.to_vec())],
        ),
    }
}

#[inline(never)]
fn combine_gbigrat(e: &Expr, rat: &GBigRational) -> ScoredTerm<Expr> {
    ScoredTerm {
        term: e.clone(),
        score: Tree::<ScoreAtom>::create_node_from_i32(
            Score::BIG_RAT,
            vec![
                Tree::<ScoreAtom>::create_leaf_from_bytes(rat.numerator.clone()),
                Tree::<ScoreAtom>::create_leaf_from_bytes(rat.denominator.clone()),
            ],
        ),
    }
}

#[inline(never)]
fn combine_gfixedpoint(e: &Expr, fp: &GFixedPoint) -> ScoredTerm<Expr> {
    ScoredTerm {
        term: e.clone(),
        score: Tree::<ScoreAtom>::create_node_from_i32(
            Score::FIXED_POINT,
            vec![
                Tree::<ScoreAtom>::create_leaf_from_bytes(fp.unscaled.clone()),
                Tree::<ScoreAtom>::create_node_from_i64s(vec![fp.scale as i64]),
            ],
        ),
    }
}

/// The `expr_instance: None` arm.
///
// TODO get rid of Empty nodes in Protobuf unless they represent sth indeed optional - OLD
#[inline(never)]
fn combine_expr_absent(e: &Expr) -> ScoredTerm<Expr> {
    ScoredTerm {
        term: e.clone(),
        score: Tree::<ScoreAtom>::create_node_from_i32(Score::ABSENT, Vec::new()),
    }
}

/// `ExprSortMatcher::sort_match`'s post-order half — the **dispatcher**.
///
/// Children are those listed by [`expr_child_pars`], in the same order. Every
/// arm is an out-of-line `#[inline(never)]` function, so this function's own
/// frame holds only the discriminant and the call; see the section header above
/// for the measurement that forced that.
///
/// ⚠ The `ESetBody`, `EMapBody` and `EPathmapBody` arms take **no** children
/// and run their own bounded drives; see the module docs for why, and
/// `stack_depth_gate.rs`'s `sort_nested_set` for the measured residual.
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

            // ---- ⚠ SELF-CONTAINED ARMS (module docs) ----
            ExprInstance::EMapBody(emap) => combine_emap(emap),
            ExprInstance::ESetBody(eset) => combine_eset(eset),
            ExprInstance::EPathmapBody(pathmap) => combine_epathmap(pathmap),

            // ---- grounds ----
            ExprInstance::GBool(gb) => combine_gbool(e, *gb),
            ExprInstance::GInt(gi) => combine_gint(e, *gi),
            ExprInstance::GString(gs) => combine_gstring(e, gs),
            ExprInstance::GUri(gu) => combine_guri(e, gu),
            ExprInstance::GByteArray(ba) => combine_gbytearray(e, ba),
            ExprInstance::GDouble(bits) => combine_gdouble(e, *bits),
            ExprInstance::GBigInt(bytes) => combine_gbigint(e, bytes),
            ExprInstance::GBigRat(rat) => combine_gbigrat(e, rat),
            ExprInstance::GFixedPoint(fp) => combine_gfixedpoint(e, fp),
        },
        None => combine_expr_absent(e),
    }
}


// ===========================================================================
// the three SELF-CONTAINED `Expr` arms
// ===========================================================================
//
// ⚠ These are the arms that re-enter `ParSortMatcher::sort_match` on OWNED
// intermediates and therefore keep one frame alive per nesting level while an
// inner drive runs. `#[inline(never)]` and out-of-line so that the frame on
// that chain is this arm's own locals and NOT the whole 36-arm frame of
// `combine_expr` (at `-O0` rustc does not overlap mutually exclusive match
// arms' stack slots — the same effect that makes every constant in the audit
// 2–12× larger in debug than in release).
//
// The residual that remains is the DERIVED class and cannot be removed by any
// conversion of the sorter: `HashSet<Par>` / `HashMap<Par, Par>` invoke
// `Par: Clone + Hash + Eq`, each a derived recursive traversal
// (`<Par as Clone>::clone` measured at 15,875 B/level, debug). The gate
// measures `sort_nested_set` against `clone_nested_set` — its own derived
// control — so the residual is attributed rather than assumed.
//
// ⚠ A second, independent property of these arms, recorded because it bounds
// how much the stack residual can ever matter: each sorts its elements THREE
// times (once inside `eset_to_par_set` / `emap_to_par_map`, once to score
// them, once inside the `create_from_vec` that rebuilds the collection), so a
// chain of `n` nested sets costs `3^n` sorts. That is pre-existing, it is a
// TIME bound rather than a stack bound, and it makes deep nesting infeasible
// long before the stack residual could bite: depth 20 is 3.5e9 sorts. It is
// named here rather than fixed because collapsing the rounds would change the
// number and order of `HashSet` constructions, which is observable whenever
// two distinct elements have equal scores (see the module docs).

/// The `EMapBody` arm.
#[inline(never)]
fn combine_emap(emap: &crate::rhoapi::EMap) -> ScoredTerm<Expr> {
let par_map = ParMapTypeMapper::emap_to_par_map(emap.clone());

fn sort_key_value_pair(key: &Par, value: &Par) -> ScoredTerm<(Par, Par)> {
    let sorted_key = ParSortMatcher::sort_match(key);
    let sorted_value = ParSortMatcher::sort_match(value);

    ScoredTerm {
        term: (sorted_key.term, sorted_value.term),
        score: sorted_key.score,
    }
}

let sorted_pars: Vec<ScoredTerm<(Par, Par)>> = par_map
    .ps
    .sorted_list
    .iter()
    .map(|kv| sort_key_value_pair(&kv.0, &kv.1))
    .collect();

let remainder_score = remainder_score(&par_map.remainder);
let connective_used_score: i64 = flag_score(par_map.connective_used);
let (pair_terms, pair_scores) = split_scored_terms(sorted_pars);

construct_expr(
    ExprInstance::EMapBody(ParMapTypeMapper::par_map_to_emap(ParMap::new(
        pair_terms,
        par_map.connective_used,
        par_map.locally_free,
        par_map.remainder,
    ))),
    Tree::Node(
        vec![
            Tree::<ScoreAtom>::create_leaf_from_i64(Score::EMAP as i64),
            remainder_score,
        ]
        .into_iter()
        .chain(pair_scores)
        .chain(std::iter::once(Tree::<ScoreAtom>::create_leaf_from_i64(
            connective_used_score,
        )))
        .collect(),
    ),
)
            
}

// ═══════════════════════════════════════════════════════════════════════════════════════
// ★★★ THE IDENTICAL-TOTAL-ORDER ARGUMENT — required BEFORE converting these three arms
// ═══════════════════════════════════════════════════════════════════════════════════════
//
// `ESetBody`, `EMapBody` and `EPathmapBody` each re-enter `ParSortMatcher::sort_match` per
// element — one native frame per nesting level. Converting them to the work-stack driver is
// stack-safety Phase 3b, and this block is the argument that obligation demands, recorded
// BEFORE any code. Sorting is ORDER-DEFINING, so the usual "evaluation order is unobservable"
// reasoning does not apply, and `SortedParMap` feeds the canonical sort that
// `cost_accounting/sig.rs` signs. **A conversion that reproduces the order ALMOST exactly is a
// fork.**
//
// ── What actually establishes the order, read from this file ──────────────────────────
//
// It is NOT this arm. `combine_eset` never sorts by score. It:
//
//   1. maps each element through `sort_match`, in the ITERATION ORDER of
//      `par_set.ps.sorted_pars`;
//   2. `split_scored_terms` splits that into `(element_terms, element_scores)`, preserving
//      that order in both;
//   3. hands `element_terms` to `SortedParHashSet::create_from_vec` — and THE CONTAINER
//      establishes the term order;
//   4. chains `element_scores` into the score `Tree`, still in the order from step 1.
//
// ⇒ ★★ **Terms and scores are ordered by two DIFFERENT things** — the terms by the
// container's constructor, the scores by the input iteration — and those are not the same
// permutation. Any conversion must preserve both.
//
// ── The three obligations a converted form must discharge ─────────────────────────────
//
// **O1 · Score order is INPUT order, and a LIFO work stack does not preserve it — but the
// driver ALREADY DISCHARGES THIS, by construction.** A work stack completes children in
// reverse push order, so a naive conversion would chain `element_scores` reversed. That
// changes the score tree, which is the sort key of the ENCLOSING term, so the corruption
// would not be local to this collection.
//
// ★★ It cannot happen here. `SortNode.idx` / `IxKont.idx` carry the **source index** through
// both stacks — `sort_drive.rs:209` documents the wrapper as existing for exactly this — and
// `SortTraversal::combine` runs
//
//     assert_children_are_in_source_order(vals, kont.arity());
//
// **before any pop**, "exactly where the bespoke loop ran it… the assertion that turns 'push
// in reverse' from a convention into a failure on every term any test sorts." The `pop_n_*`
// helpers then restore forward order and assert the indices descend by one while doing it.
//
// ⇒ O1 is **not** an obligation the conversion's author must remember; it is an invariant the
// driver fails on. ★ That is what makes 3b tractable at all — the one hazard a byte
// differential would be blind to is the one already mechanised.
//
// **O2 · The container's constructor is part of the canonical form.**
// `ParSetTypeMapper::eset_to_par_set` yields a `SortedParSet` whose order comes from
// `create_from_vec`, not from this arm. A conversion that sorts elements itself and bypasses
// the mapper would be a SECOND opinion about set order. It must keep handing terms to the
// same constructor.
//
// **O3 · `EMapBody` keeps only the KEY's score.** `sort_key_value_pair` (`:1442-1450`) sorts
// key and value and returns `score: sorted_key.score`; **the value's score is DISCARDED**.
// That asymmetry is the map's ordering rule, not an implementation detail. ⚠ It is also the
// obligation most likely to be lost in conversion, because a converted arm pushes key and
// value as two peer children and both scores are then sitting on the value stack — combining
// them is the *natural* thing to write and it is wrong. The conversion must drop the value's
// score deliberately, and say so where it does.
//
// ── Why `NodeKind` cannot express this today ──────────────────────────────────────────
//
// ⚠ `sort_drive.rs`'s `NodeKind` has twelve variants (`:190-207`) and none is `ESet`, `EMap`
// or `EPathMap`; `:1353` records the same fact from this side ("take **no** children"). The
// elements ARE `Par`s, so `NodeKind::Par` carries the descent unchanged — what is missing is
// a COMBINE that pops N scored children and routes the terms through the mapper (O2). That is
// a `SortKont` variant rather than a `NodeKind` variant, and
//
// ★ adding one is FORCED to be complete: `SortKont::arity` is exhaustive with no `_` arm, and
// the driver cross-checks it against the per-arm pop counts on every combine. A new variant
// that forgot its arity does not compile; one that mis-stated it fails the assertion. ⇒ The
// missing piece is bounded and the compiler names it.
//
// ⚠ Note what this does NOT give: arity is a count, so it pins how many children are popped,
// never that the RIGHT scores were kept (O3) or that the mapper was used (O2). Those two stay
// the human obligations, which is why they are written out above rather than left to review.
// Continuing —
// `IxKont` already carries an index.
//
// ── The falsifier — what the two existing checks CAN and CANNOT catch, measured ───────
//
// ⛔★★ **Neither existing check gates this conversion at depth ≥ 2, and for opposite reasons.**
//
// **`sort_recursive.rs` — the frozen oracle — is blind to it BY CONSTRUCTION, and says so.**
// Its header: *"The oracle shares [`super::sort_combine`] with the driver, so the two differ
// **only** in traversal shape… It follows that this differential **cannot** catch an error
// transcribed into the shared table itself — both sides would be wrong together."* ⇒ These
// three arms ARE that shared table. O2 and O3 live inside them, so converting the arms moves
// both sides of the differential at once and the check goes quiet on exactly the change being
// made. ★ This is not a flaw in the oracle — it is the design that makes it a sharp test of
// traversal shape — but it means **the oracle is not 3b's gate**, and reaching for it out of
// habit would be a vacuous pass.
//
// ★ `sorter_canonical_golden.rs` is the one that can see arm errors, and it is better than a
// byte gate: `line()` (`:207`) records **both
// columns** — `sort_match(x).term` *and* `sort_match(x).score` (header, `:29`). So it observes
// the score tree directly, and **O3's discard is visible to it**: keeping the value's score
// would add atoms to the recorded column even where the emitted bytes did not move. ⇒ Do not
// repeat the "byte gates are blind" reflex here without checking; for the score it is false.
//
// ⛔ **What it cannot catch: the recursion 3b converts.** Measured at HEAD, every collection in
// the corpus is DEPTH 1 and scalar-only —
//
//     ESetBody      ps  = [gint(9), gint(3), gstring("m")]        (`:392`)
//     EMapBody      kvs = [9→90, 3→30]                            (`:401-410`)
//     EPathmapBody  pathmap_of([gint(9), gint(3)])                (`:418`)
//
// **No element of any collection is itself a collection.** The three arms are precisely the
// ones that re-enter `sort_match` per nesting level, and the corpus never makes them re-enter
// even once. A conversion could be wrong at every level below the first and this golden would
// be byte- and score-identical.
//
// ⚠ The `EMapBody` row is additionally **monotone** — keys 3 < 9 pair with values 30 < 90 — so
// key-order and key⊕value-order agree, and O3's defect would not reorder it even at depth 1.
// Discriminating O3 by ORDER (rather than by the score column) needs an ANTI-MONOTONE pair:
// `3→90, 9→30`.
//
// ⇒ **The two blindnesses compose into one gap, and it is precisely 3b's target.** The oracle
// covers traversal shape but not the arms; the golden covers the arms but only at depth 1;
// **the arms at depth ≥ 2 are covered by neither** — and "the arms at depth ≥ 2" is the exact
// description of the recursion this conversion removes.
//
// ⇒ **The prerequisite is a corpus extension, not just a diff.** Before any arm is converted,
// `sorter_canonical_golden.rs` must carry: a set inside a map inside a set, each collection
// ≥ 2 elements, plus an anti-monotone map (`3→90, 9→30`), captured from the PRE-conversion
// implementation exactly as the existing rows were. ★ A one-element collection has exactly one
// permutation, so it cannot separate any ordering hypothesis from any other; a depth-1 corpus
// cannot separate a correct conversion from one that is right only at the root.
//
// ⚠ Capture order matters and is not recoverable later: the golden's authority comes from
// being taken **before** the change. Extending it afterwards would pin whatever the conversion
// happened to produce — a fixture that agrees with the code by construction, which is the
// vacuous-gate shape this campaign has now hit three times.
//
// ★ Precedent for taking this seriously rather than as a formality: `69e67043` deleted a push
// from `contains_par` and all 30 byte-gate tests stayed green, and this campaign then hit the
// same shape again — an unsound memoisation caught by the predicate oracle while every byte
// gate stayed green.

/// The `ESetBody` arm.
#[inline(never)]
fn combine_eset(eset: &crate::rhoapi::ESet) -> ScoredTerm<Expr> {
let par_set = ParSetTypeMapper::eset_to_par_set(eset.clone());
let sorted_pars: Vec<ScoredTerm<Par>> = par_set
    .ps
    .sorted_pars
    .iter()
    .map(ParSortMatcher::sort_match)
    .collect();

let remainder_score = remainder_score(&par_set.remainder);
let connective_used_score: i64 = flag_score(par_set.connective_used);
let (element_terms, element_scores) = split_scored_terms(sorted_pars);

construct_expr(
    ExprInstance::ESetBody(ParSetTypeMapper::par_set_to_eset(ParSet {
        ps: SortedParHashSet::create_from_vec(element_terms),
        connective_used: par_set.connective_used,
        locally_free: par_set.locally_free,
        remainder: par_set.remainder,
    })),
    Tree::Node(
        vec![
            Tree::<ScoreAtom>::create_leaf_from_i64(Score::ESET as i64),
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

/// The `EPathmapBody` arm.
#[inline(never)]
fn combine_epathmap(pathmap: &EPathMap) -> ScoredTerm<Expr> {
// ★ ENTRY ORDER IS NO LONGER THIS FUNCTION'S BUSINESS.
// An `EPathMap` stores its entries in a trie, so what
// `pathmap.ps()` hands back is already trie order,
// deduplicated, and recursively canonical. The GROUND arm
// therefore has nothing left to do — `canonicalize_ground_epathmap`
// was exactly this projection, and it is deleted.
//
// The NON-GROUND arm still has real work, and it is NOT
// ordering: a non-ground entry can carry an AC collection
// (`ESet`/`EMap`) that only `sort_match` can normalize, and
// the codec's escape arm files such an entry by its raw prost
// bytes, so the projection returns it un-normalized. Sorting
// an entry CHANGES ITS KEY, which is why the sorted entries
// are handed to `EPathMap::new` — re-filing them puts the
// normalized set back in trie order and merges any two entries
// that AC-normalized to the same term.
//
// ⚠ Consensus-visible: this arm used to PRESERVE ENTRY ORDER
// (`401ed168` measured it). It no longer can — there is no
// order to preserve.
let canonical = if eval_stable_epathmap(pathmap) && !pathmap.ps().is_empty() {
    pathmap.clone()
} else {
    EPathMap::new(
        pathmap
            .ps()
            .iter()
            .map(|p| ParSortMatcher::sort_match(p).term)
            .collect::<Vec<Par>>(),
        pathmap.locally_free.clone(),
        pathmap.connective_used,
        pathmap.remainder.clone(),
    )
};
// Score the (now-canonical) entries so the enclosing sort
// agrees with the emitted term order.
let pars: Vec<ScoredTerm<Par>> = canonical
    .ps()
    .iter()
    .map(ParSortMatcher::sort_match)
    .collect();
let remainder_score = remainder_score(&canonical.remainder);
let connective_used_score: i64 = flag_score(canonical.connective_used);

construct_expr(
    ExprInstance::EPathmapBody(canonical),
    Tree::Node(
        vec![
            Tree::<ScoreAtom>::create_leaf_from_i64(Score::EPATHMAP as i64),
            remainder_score,
        ]
        .into_iter()
        .chain(pars.into_iter().map(|p| p.score))
        .chain(std::iter::once(Tree::<ScoreAtom>::create_leaf_from_i64(
            connective_used_score,
        )))
        .collect(),
    ),
)
            
}
