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
//! ## Collection arms are worklisted and deterministic
//!
//! `ESetBody` and `EMapBody` expose raw members after applying their
//! collection-level duplicate semantics. Homogeneous EPathMap modes enumerate
//! entries from PathMap in trie order; set keys are decoded into traversal-arena
//! storage and map values remain borrowed from the trie. Their combines consume
//! already-scored children, canonical-sort with the total term order, and never
//! re-enter `ParSortMatcher`. Randomized HashMap iteration cannot affect the
//! result because every unordered intermediate is followed by that total sort.
//! The converted nested-set/map subjects run through the permanent full-depth
//! gate.
//!
//! Full analysis, measured constants and proof standard:
//! `docs/design/audits/theta-depth-traversals-2026-07-26.md`.

use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::sync::OnceLock;

use super::score_tree::{EmittedBytes, Score, ScoreAtom, ScoredTerm, Tree};
use crate::rhoapi::connective::ConnectiveInstance;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::g_unforgeable::UnfInstance;
use crate::rhoapi::var::VarInstance;
use crate::rhoapi::{
    Bundle, Connective, CostTokenStack, EAnd, EDiv, EEq, EGt, EGte, EList, ELt, ELte, EMatches,
    EMethod, EMinus, EMinusMinus, EMod, EMult, ENeg, ENeq, ENot, EOr, EPathMap, EPercentPercent,
    EPlus, EPlusPlus, ETuple, EVar, EZipper, Expr, GBigRational, GFixedPoint, GUnforgeable, If,
    Match, MatchCase, New, Par, Receive, ReceiveBind, Send, Var,
};
use crate::rust::canonical_path::encode_trie_path_with_stability;
use crate::rust::epathmap_trie_codec::EPathMapMode;
use crate::rust::pathmap_crate_type_mapper::eval_stable_epathmap;
use crate::rust::rhoapi_ext::EntryTrie;
use crate::rust::rholang::canonical_semantics::{self as semantics, CanonicalScoreAlgebra};

struct TreeScoreAlgebra;

impl CanonicalScoreAlgebra for TreeScoreAlgebra {
    type Score = Tree<ScoreAtom>;
    type Error = Infallible;

    fn integer(&mut self, value: i64) -> Result<Self::Score, Self::Error> {
        Ok(Tree::<ScoreAtom>::create_leaf_from_i64(value))
    }

    fn string(&mut self, value: &str) -> Result<Self::Score, Self::Error> {
        Ok(Tree::<ScoreAtom>::create_leaf_from_string(value.to_owned()))
    }

    fn bytes(&mut self, value: &[u8]) -> Result<Self::Score, Self::Error> {
        Ok(Tree::<ScoreAtom>::create_leaf_from_bytes(value.to_vec()))
    }

    fn node(&mut self, children: Vec<Self::Score>) -> Result<Self::Score, Self::Error> {
        Ok(Tree::Node(children))
    }
}

fn semantic_score(
    build: impl FnOnce(&mut TreeScoreAlgebra) -> Result<Tree<ScoreAtom>, Infallible>,
) -> Tree<ScoreAtom> {
    match build(&mut TreeScoreAlgebra) {
        Ok(score) => score,
        Err(error) => match error {},
    }
}

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
    let score = match &v.var_instance {
        Some(VarInstance::BoundVar(level)) => semantics::VarScore::Bound(*level),
        Some(VarInstance::FreeVar(level)) => semantics::VarScore::Free(*level),
        Some(VarInstance::Wildcard(_)) => semantics::VarScore::Wildcard,
        None => semantics::VarScore::Absent,
    };
    ScoredTerm {
        term: if v.var_instance.is_some() {
            v.clone()
        } else {
            Var::default()
        },
        score: semantic_score(|algebra| semantics::var_score(algebra, score)),
    }
}

/// `UnforgeableSortMatcher::sort_match`, verbatim. Also a leaf: every arm
/// carries bytes, never a `Par`.
pub fn sort_unforgeable(unf: &GUnforgeable) -> ScoredTerm<GUnforgeable> {
    let score = match &unf.unf_instance {
        Some(UnfInstance::GPrivateBody(value)) => semantics::UnforgeableScore::Private(&value.id),
        Some(UnfInstance::GDeployerIdBody(value)) => {
            semantics::UnforgeableScore::DeployerId(&value.public_key)
        }
        Some(UnfInstance::GDeployIdBody(value)) => {
            semantics::UnforgeableScore::DeployId(&value.sig)
        }
        Some(UnfInstance::GSysAuthTokenBody(_)) => semantics::UnforgeableScore::SysAuthToken,
        None => semantics::UnforgeableScore::Absent,
    };
    ScoredTerm {
        term: unf.clone(),
        score: semantic_score(|algebra| semantics::unforgeable_score(algebra, score)),
    }
}

pub fn sort_cost_token_stack(stack: &CostTokenStack) -> ScoredTerm<CostTokenStack> {
    ScoredTerm {
        term: stack.clone(),
        score: semantic_score(|algebra| semantics::cost_token_stack_score(algebra, stack)),
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
/// `ESetBody` and `EMapBody` expose their raw-container members after applying
/// the same raw equality semantics as their mathematical collection. PathMap
/// keys remain a separate trie traversal because set keys must be decoded into
/// arena-owned `Par`s; the generated driver and recursive oracle each supply
/// that storage explicitly.
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

        ExprInstance::ESetBody(eset) => extend_eset_children(eset, out),
        ExprInstance::EMapBody(emap) => extend_emap_children(emap, out),
        ExprInstance::EPathmapBody(_) | ExprInstance::EZipperBody(_) => {}

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

fn extend_eset_children<'t>(eset: &'t crate::rhoapi::ESet, out: &mut Vec<&'t Par>) {
    if eset.ps.len() <= 1 {
        out.extend(eset.ps.iter());
        return;
    }
    let unique: HashSet<&Par> = eset.ps.iter().collect();
    out.reserve(unique.len());
    out.extend(unique);
}

fn extend_emap_children<'t>(emap: &'t crate::rhoapi::EMap, out: &mut Vec<&'t Par>) {
    if let [pair] = emap.kvs.as_slice() {
        out.push(pair.key.as_ref().expect("KeyValuePair.key"));
        out.push(pair.value.as_ref().expect("KeyValuePair.value"));
        return;
    }
    let mut unique: HashMap<&Par, &Par> = HashMap::with_capacity(emap.kvs.len());
    for pair in &emap.kvs {
        let key = pair.key.as_ref().expect("KeyValuePair.key");
        let value = pair.value.as_ref().expect("KeyValuePair.value");
        unique
            .entry(key)
            .and_modify(|current| {
                // `Par` equality is canonical PathMap equality. Two source
                // keys can therefore compare equal even when their former
                // repeated-field spellings differed. Preserve the exact
                // winner the complete two-stage sorter obtains. This first
                // stage retains the first equality-representative key and the
                // value with greatest emitted bytes. The distinction matters
                // for hostile decoded Pars whose keys differ only in the root
                // `locally_free` cache ignored by `Par` equality: the key
                // representative is still the first source occurrence. The
                // second stage sorts the surviving complete pairs by `(key
                // score, KeyValuePair bytes)`. HashMap seeding cannot affect
                // either choice, and bytes are allocated only on a real
                // collision.
                if value.emitted_bytes() > current.emitted_bytes() {
                    *current = value;
                }
            })
            .or_insert(value);
    }
    out.reserve(unique.len() * 2);
    for (key, value) in unique {
        out.push(key);
        out.push(value);
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
pub fn combine_par(par: &Par, _parts: ParParts) -> ScoredTerm<Par> {
    // ParBag insertion canonicalizes every typed component and stores its score
    // key before the canonical protobuf body.  Re-running nine child sorters
    // here would decode and rebuild a value that is already canonical.
    ScoredTerm {
        term: par.clone(),
        score: par.canonical_score(),
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
        cost_region: s.cost_region.clone(),
    };

    let send_score = semantic_score(|algebra| {
        let region_score = s
            .cost_region
            .as_ref()
            .map(|region| semantics::cost_region_score(algebra, region))
            .transpose()?;
        semantics::send_score(
            algebra,
            s.persistent,
            chan.score,
            data_scores,
            s.connective_used,
            region_score,
        )
    });

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
            cost_region: bind.cost_region.clone(),
        },
        score: semantic_score(|algebra| {
            let region_score = bind
                .cost_region
                .as_ref()
                .map(|region| semantics::cost_region_score(algebra, region))
                .transpose()?;
            semantics::receive_bind_score(
                algebra,
                channel.score,
                pattern_scores,
                sorted_remainder_score,
                region_score,
            )
        }),
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
            cost_region: r.cost_region.clone(),
        },
        score: semantic_score(|algebra| {
            let region_score = r
                .cost_region
                .as_ref()
                .map(|region| semantics::cost_region_score(algebra, region))
                .transpose()?;
            semantics::receive_score(
                algebra,
                r.persistent,
                r.peek,
                bind_scores,
                body.score,
                r.bind_count,
                r.connective_used,
                condition_score,
                region_score,
            )
        }),
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
        vec![Tree::<ScoreAtom>::create_leaf_from_i64(
            Score::ABSENT as i64,
        )]
    };

    let injections_score: Vec<Tree<ScoreAtom>> = if !n.injections.is_empty() {
        n.injections
            .keys()
            .zip(injections)
            .map(|(k, scored)| {
                semantic_score(|algebra| semantics::injection_score(algebra, k, scored.score))
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
        score: semantic_score(|algebra| {
            semantics::new_score(algebra, n.bind_count, uri_score, injections_score, p.score)
        }),
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
        score: semantic_score(|algebra| {
            semantics::match_case_score(
                algebra,
                pattern.score,
                source.score,
                match_case.free_count,
                guard_score,
            )
        }),
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
        score: semantic_score(|algebra| {
            semantics::match_score(algebra, target.score, case_scores, m.connective_used)
        }),
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
        score: semantic_score(|algebra| {
            semantics::if_score(
                algebra,
                condition.score,
                if_true.score,
                if_false.score,
                i.connective_used,
            )
        }),
    }
}

/// `BundleSortMatcher::sort_match`'s post-order half. One child: `body`.
pub fn combine_bundle(b: &Bundle, body: ScoredTerm<Par>) -> ScoredTerm<Bundle> {
    ScoredTerm {
        term: {
            let mut b_cloned = b.clone();
            b_cloned.body = Some(body.term);
            b_cloned
        },
        score: semantic_score(|algebra| {
            semantics::bundle_score(algebra, b.read_flag, b.write_flag, body.score)
        }),
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
                score: semantic_score(|algebra| {
                    semantics::connective_score(algebra, semantics::ConnectiveScore::And(scores))
                }),
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
                score: semantic_score(|algebra| {
                    semantics::connective_score(algebra, semantics::ConnectiveScore::Or(scores))
                }),
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
                score: semantic_score(|algebra| {
                    semantics::connective_score(
                        algebra,
                        semantics::ConnectiveScore::Not(scored_par.score),
                    )
                }),
            }
        }

        Some(ConnectiveInstance::VarRefBody(v)) => ScoredTerm {
            term: Connective {
                connective_instance: Some(ConnectiveInstance::VarRefBody(v.clone())),
            },
            score: semantic_score(|algebra| {
                semantics::connective_score(algebra, semantics::ConnectiveScore::VarRef {
                    index: v.index,
                    depth: v.depth,
                })
            }),
        },

        Some(ConnectiveInstance::ConnBool(b)) => ScoredTerm {
            term: Connective {
                connective_instance: Some(ConnectiveInstance::ConnBool(*b)),
            },
            score: semantic_score(|algebra| {
                semantics::connective_score(algebra, semantics::ConnectiveScore::Bool(*b))
            }),
        },

        Some(ConnectiveInstance::ConnInt(b)) => ScoredTerm {
            term: Connective {
                connective_instance: Some(ConnectiveInstance::ConnInt(*b)),
            },
            score: semantic_score(|algebra| {
                semantics::connective_score(algebra, semantics::ConnectiveScore::Int(*b))
            }),
        },

        Some(ConnectiveInstance::ConnString(b)) => ScoredTerm {
            term: Connective {
                connective_instance: Some(ConnectiveInstance::ConnString(*b)),
            },
            score: semantic_score(|algebra| {
                semantics::connective_score(algebra, semantics::ConnectiveScore::String(*b))
            }),
        },

        Some(ConnectiveInstance::ConnUri(b)) => ScoredTerm {
            term: Connective {
                connective_instance: Some(ConnectiveInstance::ConnUri(*b)),
            },
            score: semantic_score(|algebra| {
                semantics::connective_score(algebra, semantics::ConnectiveScore::Uri(*b))
            }),
        },

        Some(ConnectiveInstance::ConnByteArray(b)) => ScoredTerm {
            term: Connective {
                connective_instance: Some(ConnectiveInstance::ConnByteArray(*b)),
            },
            score: semantic_score(|algebra| {
                semantics::connective_score(algebra, semantics::ConnectiveScore::ByteArray(*b))
            }),
        },

        None => ScoredTerm {
            term: Connective::default(),
            score: semantic_score(|algebra| {
                semantics::connective_score(algebra, semantics::ConnectiveScore::Absent)
            }),
        },
    }
}

// ===========================================================================
// `Expr` — the DISPATCHER and its thirty-six per-arm functions
// ===========================================================================
//
// ## Why every arm is its own `#[inline(never)]` function
//
// At `-O0` rustc does **not** overlap the stack slots of mutually exclusive
// `match` arms. A single function carrying all 36 `ExprInstance` arms' locals
// therefore costs a frame the size of their *sum*: measured by `gdb` on the
// draft of this conversion, `combine_expr` was **99,888 bytes** — larger than
// the whole recursive `ExprSortMatcher::sort_match` it replaced (69,744 B).
//
// The driver is flat, but a needlessly large constant frame still wastes the
// small explicit stacks used by the regression gates. Splitting all arms keeps
// each frame bounded by only that arm's locals and preserves the exhaustiveness
// of the dispatcher.

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
    ($name:ident, $variant:ident, $ctor:ident) => {
        #[inline(never)]
        fn $name(constructor: i32, kids: Vec<ScoredTerm<Par>>) -> ScoredTerm<Expr> {
            let ScoredTerm { term, score } = one(kids);
            construct_expr(
                ExprInstance::$variant($ctor { p: Some(term) }),
                semantic_score(|algebra| semantics::unary_score(algebra, constructor, score)),
            )
        }
    };
}

/// `p1 OP p2`: two child `Par`s in slot order, scored `node_i32(tag, [s1, s2])`.
macro_rules! binary_arm {
    ($name:ident, $variant:ident, $ctor:ident) => {
        #[inline(never)]
        fn $name(constructor: i32, kids: Vec<ScoredTerm<Par>>) -> ScoredTerm<Expr> {
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
                semantic_score(|algebra| semantics::binary_score(algebra, constructor, s1, s2)),
            )
        }
    };
}

unary_arm!(combine_eneg, ENegBody, ENeg);
unary_arm!(combine_enot, ENotBody, ENot);

binary_arm!(combine_emult, EMultBody, EMult);
binary_arm!(combine_ediv, EDivBody, EDiv);
binary_arm!(combine_emod, EModBody, EMod);
binary_arm!(combine_eplus, EPlusBody, EPlus);
binary_arm!(combine_eminus, EMinusBody, EMinus);
binary_arm!(combine_elt, ELtBody, ELt);
binary_arm!(combine_elte, ELteBody, ELte);
binary_arm!(combine_egt, EGtBody, EGt);
binary_arm!(combine_egte, EGteBody, EGte);
binary_arm!(combine_eeq, EEqBody, EEq);
binary_arm!(combine_eneq, ENeqBody, ENeq);
binary_arm!(combine_eand, EAndBody, EAnd);
binary_arm!(combine_eor, EOrBody, EOr);
binary_arm!(
    combine_epercentpercent,
    EPercentPercentBody,
    EPercentPercent
);
binary_arm!(combine_eplusplus, EPlusPlusBody, EPlusPlus);
binary_arm!(combine_eminusminus, EMinusMinusBody, EMinusMinus);

/// `EVarBody`. Its child is a `Var` — a leaf — so it takes no worklist
/// children and scores in place.
#[inline(never)]
fn combine_evar(ev: &EVar) -> ScoredTerm<Expr> {
    let sorted_var = sort_var(ev.v.as_ref().expect("var field was None, should be Some"));
    construct_expr(
        ExprInstance::EVarBody(EVar {
            v: Some(sorted_var.term),
        }),
        semantic_score(|algebra| semantics::unary_score(algebra, Score::EVAR, sorted_var.score)),
    )
}

/// `EMatchesBody`. Slot order is `target`, then `pattern`.
#[inline(never)]
fn combine_ematches(constructor: i32, kids: Vec<ScoredTerm<Par>>) -> ScoredTerm<Expr> {
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
        semantic_score(|algebra| {
            semantics::binary_score(algebra, constructor, target_score, pattern_score)
        }),
    )
}

/// `EListBody`. Order-preserving: the emitted `ps` is the input order.
#[inline(never)]
fn combine_elist(list: &EList, kids: Vec<ScoredTerm<Par>>) -> ScoredTerm<Expr> {
    let remainder_score = remainder_score(&list.remainder);
    let (element_terms, element_scores) = split_scored_terms(kids);

    construct_expr(
        ExprInstance::EListBody(EList {
            ps: element_terms,
            locally_free: list.locally_free.clone(),
            connective_used: list.connective_used,
            remainder: list.remainder.clone(),
        }),
        semantic_score(|algebra| {
            semantics::ordered_collection_score(
                algebra,
                Score::ELIST,
                Some(remainder_score),
                element_scores,
                list.connective_used,
            )
        }),
    )
}

/// `ETupleBody`. ⚠ No remainder slot, unlike `EListBody`.
#[inline(never)]
fn combine_etuple(tuple: &ETuple, kids: Vec<ScoredTerm<Par>>) -> ScoredTerm<Expr> {
    let (element_terms, element_scores) = split_scored_terms(kids);
    let mut tuple_cloned = tuple.clone();
    tuple_cloned.ps = element_terms;

    construct_expr(
        ExprInstance::ETupleBody(tuple_cloned),
        semantic_score(|algebra| {
            semantics::ordered_collection_score(
                algebra,
                Score::ETUPLE,
                None,
                element_scores,
                tuple.connective_used,
            )
        }),
    )
}
/// Rebuild one homogeneous EPathMap from already-sorted PDA children and
/// return score trees in the rebuilt trie's canonical order.
///
/// Canonical terms are inserted directly into the specialized EntryTrie. A
/// companion PathMap keyed by the same canonical bytes carries scores until the
/// final trie-order walk; no set-member or key/value Vec projection is built.
fn canonicalize_scored_pathmap(
    pathmap: &EPathMap,
    kids: Vec<ScoredTerm<Par>>,
    preserve_stable_storage: bool,
) -> (EPathMap, Vec<Tree<ScoreAtom>>) {
    if preserve_stable_storage && eval_stable_epathmap(pathmap) && !pathmap.is_empty() {
        return (
            pathmap.clone(),
            kids.into_iter().map(|child| child.score).collect(),
        );
    }

    let metadata = || {
        (
            pathmap.locally_free.clone(),
            pathmap.connective_used,
            pathmap.remainder.clone(),
        )
    };

    match pathmap.mode() {
        EPathMapMode::Empty => {
            assert!(kids.is_empty(), "empty EPathMap has no sorter children");
            let (locally_free, connective_used, remainder) = metadata();
            (
                EPathMap::new(
                    EntryTrie::default(),
                    locally_free,
                    connective_used,
                    remainder,
                ),
                Vec::new(),
            )
        }
        EPathMapMode::Set => {
            let mut entries = EntryTrie::default();
            let mut score_by_key = pathmap::PathMap::new();
            for ScoredTerm { term, score } in kids {
                let (key, stable) = encode_trie_path_with_stability(&term);
                score_by_key.insert(&key, score);
                let consumed = entries.insert_encoded(key, stable, term);
                crate::rust::rholang::par_children::dismantle(consumed);
            }

            let (locally_free, connective_used, remainder) = metadata();
            let canonical = EPathMap::new(entries, locally_free, connective_used, remainder);
            let mut entry_scores = Vec::with_capacity(canonical.len());
            canonical
                .entry_trie()
                .for_each_raw_set_entry(|key| {
                    entry_scores.push(
                        score_by_key
                            .remove(key)
                            .expect("every canonical set key came from a sorted child"),
                    );
                })
                .expect("set-mode construction cannot select map storage");
            (canonical, entry_scores)
        }
        EPathMapMode::Map => {
            assert_eq!(
                kids.len() % 2,
                0,
                "map-mode sorter produces one value for every key"
            );
            let mut children = kids.into_iter();
            let mut entries = EntryTrie::default();
            let mut score_by_key = pathmap::PathMap::new();

            while let Some(ScoredTerm {
                term: key,
                score: key_score,
            }) = children.next()
            {
                let ScoredTerm {
                    term: value,
                    score: value_score,
                } = children
                    .next()
                    .expect("map-mode sorter produces one value for every key");
                let (encoded_key, key_stable) = encode_trie_path_with_stability(&key);
                score_by_key.insert(&encoded_key, (key_score, value_score));
                if let Some(replaced) = entries
                    .insert_encoded_map_entry_replacing(&encoded_key, key_stable, key, value)
                    .expect("fresh map-mode construction cannot contain set entries")
                {
                    crate::rust::rholang::par_children::dismantle(replaced);
                }
            }

            let (locally_free, connective_used, remainder) = metadata();
            let canonical = EPathMap::new(entries, locally_free, connective_used, remainder);
            let mut entry_scores = Vec::with_capacity(canonical.len() * 2);
            canonical
                .entry_trie()
                .for_each_raw_map_entry(|key, _| {
                    let (key_score, value_score) = score_by_key
                        .remove(key)
                        .expect("every canonical map key came from a sorted child pair");
                    entry_scores.push(key_score);
                    entry_scores.push(value_score);
                })
                .expect("map-mode construction cannot select set storage");
            (canonical, entry_scores)
        }
    }
}
/// `EZipperBody`. Scores under `EPATHMAP + 1`; no remainder slot.
#[inline(never)]
fn combine_ezipper(zipper: &EZipper, kids: Vec<ScoredTerm<Par>>) -> ScoredTerm<Expr> {
    let pathmap = zipper.pathmap.as_ref().expect("zipper pathmap was None");
    let (canonical, element_scores) = canonicalize_scored_pathmap(pathmap, kids, false);

    construct_expr(
        ExprInstance::EZipperBody(EZipper {
            // Use the invariant-preserving constructor; homogeneous trie storage is private.
            pathmap: Some(canonical),
            current_path: zipper.current_path.clone(),
            is_write_zipper: zipper.is_write_zipper,
            locally_free: zipper.locally_free.clone(),
            connective_used: zipper.connective_used,
            // The cursor's split/bare discriminator travels with
            // `current_path`: sorting canonicalizes the zipper's MAP, never
            // its focus.
            cursor_kind: zipper.cursor_kind,
        }),
        semantic_score(|algebra| {
            semantics::ordered_collection_score(
                algebra,
                semantics::rpb3_pathmap_constructor(true),
                None,
                element_scores,
                zipper.connective_used,
            )
        }),
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
    let (arg_terms, arg_scores) = split_scored_terms(kids);

    let mut em_cloned = em.clone();
    em_cloned.arguments = arg_terms;
    em_cloned.target = Some(sorted_target.term);

    construct_expr(
        ExprInstance::EMethodBody(em_cloned),
        semantic_score(|algebra| {
            semantics::method_score(
                algebra,
                &em.method_name,
                sorted_target.score,
                arg_scores,
                em.connective_used,
            )
        }),
    )
}

// ---- grounds: the term is `e.clone()` (the whole `Expr`), never rebuilt ----

/// ⚠ `GBool` scores `[BOOL, 0]` when the value is **true** and `[BOOL, 1]` when
/// it is false. That inversion is in the pre-conversion matcher (and in
/// `BoolSortMatcher.scala`) and is reproduced verbatim: it is part of the
/// canonical form, and the canonical form is signed.
#[inline(never)]
fn combine_gbool(e: &Expr, gb: bool) -> ScoredTerm<Expr> {
    ScoredTerm {
        term: e.clone(),
        score: semantic_score(|algebra| {
            semantics::ground_score(algebra, semantics::GroundScore::Bool(gb))
        }),
    }
}

#[inline(never)]
fn combine_gint(e: &Expr, gi: i64) -> ScoredTerm<Expr> {
    ScoredTerm {
        term: e.clone(),
        score: semantic_score(|algebra| {
            semantics::ground_score(algebra, semantics::GroundScore::Int(gi))
        }),
    }
}

#[inline(never)]
fn combine_gstring(e: &Expr, gs: &str) -> ScoredTerm<Expr> {
    ScoredTerm {
        term: e.clone(),
        score: semantic_score(|algebra| {
            semantics::ground_score(algebra, semantics::GroundScore::String(gs))
        }),
    }
}

#[inline(never)]
fn combine_guri(e: &Expr, gu: &str) -> ScoredTerm<Expr> {
    ScoredTerm {
        term: e.clone(),
        score: semantic_score(|algebra| {
            semantics::ground_score(algebra, semantics::GroundScore::Uri(gu))
        }),
    }
}

#[inline(never)]
fn combine_gbytearray(e: &Expr, ba: &[u8]) -> ScoredTerm<Expr> {
    ScoredTerm {
        term: e.clone(),
        score: semantic_score(|algebra| {
            semantics::ground_score(algebra, semantics::GroundScore::ByteArray(ba))
        }),
    }
}

#[inline(never)]
fn combine_gdouble(e: &Expr, bits: u64) -> ScoredTerm<Expr> {
    ScoredTerm {
        term: e.clone(),
        score: semantic_score(|algebra| {
            semantics::ground_score(algebra, semantics::GroundScore::Double(bits))
        }),
    }
}

#[inline(never)]
fn combine_gbigint(e: &Expr, bytes: &[u8]) -> ScoredTerm<Expr> {
    ScoredTerm {
        term: e.clone(),
        score: semantic_score(|algebra| {
            semantics::ground_score(algebra, semantics::GroundScore::BigInt(bytes))
        }),
    }
}

#[inline(never)]
fn combine_gbigrat(e: &Expr, rat: &GBigRational) -> ScoredTerm<Expr> {
    ScoredTerm {
        term: e.clone(),
        score: semantic_score(|algebra| {
            semantics::ground_score(algebra, semantics::GroundScore::BigRat {
                numerator: &rat.numerator,
                denominator: &rat.denominator,
            })
        }),
    }
}

#[inline(never)]
fn combine_gfixedpoint(e: &Expr, fp: &GFixedPoint) -> ScoredTerm<Expr> {
    ScoredTerm {
        term: e.clone(),
        score: semantic_score(|algebra| {
            semantics::ground_score(algebra, semantics::GroundScore::FixedPoint {
                unscaled: &fp.unscaled,
                scale: fp.scale,
            })
        }),
    }
}

/// The `expr_instance: None` arm.
#[inline(never)]
fn combine_expr_absent(e: &Expr) -> ScoredTerm<Expr> {
    ScoredTerm {
        term: e.clone(),
        score: semantic_score(|algebra| {
            semantics::ground_score(algebra, semantics::GroundScore::AbsentExpr)
        }),
    }
}

/// `ExprSortMatcher::sort_match`'s post-order half — the **dispatcher**.
///
/// Children are those listed by [`expr_child_pars`], in the same order. Every
/// arm is an out-of-line `#[inline(never)]` function, so this function's own
/// frame holds only the discriminant and the call; see the section header above
/// for the measurement that forced that.
///
/// ESet/EMap members and homogeneous EPathMap entries arrive as ordinary scored
/// children of the same PDA; their arms perform only post-order rebuilding.
pub fn combine_expr(e: &Expr, kids: Vec<ScoredTerm<Par>>) -> ScoredTerm<Expr> {
    match &e.expr_instance {
        Some(expr) => match expr {
            ExprInstance::ENegBody(_) => combine_eneg(semantics::expr_constructor(expr), kids),
            ExprInstance::ENotBody(_) => combine_enot(semantics::expr_constructor(expr), kids),
            ExprInstance::EVarBody(ev) => combine_evar(ev),

            ExprInstance::EMultBody(_) => combine_emult(semantics::expr_constructor(expr), kids),
            ExprInstance::EDivBody(_) => combine_ediv(semantics::expr_constructor(expr), kids),
            ExprInstance::EModBody(_) => combine_emod(semantics::expr_constructor(expr), kids),
            ExprInstance::EPlusBody(_) => combine_eplus(semantics::expr_constructor(expr), kids),
            ExprInstance::EMinusBody(_) => combine_eminus(semantics::expr_constructor(expr), kids),
            ExprInstance::ELtBody(_) => combine_elt(semantics::expr_constructor(expr), kids),
            ExprInstance::ELteBody(_) => combine_elte(semantics::expr_constructor(expr), kids),
            ExprInstance::EGtBody(_) => combine_egt(semantics::expr_constructor(expr), kids),
            ExprInstance::EGteBody(_) => combine_egte(semantics::expr_constructor(expr), kids),
            ExprInstance::EEqBody(_) => combine_eeq(semantics::expr_constructor(expr), kids),
            ExprInstance::ENeqBody(_) => combine_eneq(semantics::expr_constructor(expr), kids),
            ExprInstance::EAndBody(_) => combine_eand(semantics::expr_constructor(expr), kids),
            ExprInstance::EOrBody(_) => combine_eor(semantics::expr_constructor(expr), kids),
            ExprInstance::EMatchesBody(_) => {
                combine_ematches(semantics::expr_constructor(expr), kids)
            }
            ExprInstance::EPercentPercentBody(_) => {
                combine_epercentpercent(semantics::expr_constructor(expr), kids)
            }
            ExprInstance::EPlusPlusBody(_) => {
                combine_eplusplus(semantics::expr_constructor(expr), kids)
            }
            ExprInstance::EMinusMinusBody(_) => {
                combine_eminusminus(semantics::expr_constructor(expr), kids)
            }

            ExprInstance::EListBody(list) => combine_elist(list, kids),
            ExprInstance::ETupleBody(tuple) => combine_etuple(tuple, kids),
            ExprInstance::EZipperBody(zipper) => combine_ezipper(zipper, kids),
            ExprInstance::EMethodBody(em) => combine_emethod(em, kids),

            ExprInstance::EMapBody(emap) => combine_emap(emap, kids),
            ExprInstance::ESetBody(eset) => combine_eset(eset, kids),
            ExprInstance::EPathmapBody(pathmap) => combine_epathmap(pathmap, kids),

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
// collection `Expr` arms
// ===========================================================================
//
// Each arm consumes children already produced by the single explicit-worklist
// traversal. ESet/EMap canonicalization therefore performs no recursive sorter
// calls, and EPathMap rebuilding stays within the homogeneous PathMap mode.

/// The `EMapBody` arm.
#[inline(never)]
fn combine_emap(emap: &crate::rhoapi::EMap, kids: Vec<ScoredTerm<Par>>) -> ScoredTerm<Expr> {
    assert_eq!(
        kids.len() % 2,
        0,
        "EMap sorter receives one value for every key"
    );

    let mut kids = kids.into_iter();
    let mut pairs = Vec::with_capacity(kids.len() / 2);
    while let Some(key) = kids.next() {
        let value = kids
            .next()
            .expect("EMap sorter receives one value for every key");
        pairs.push(ScoredTerm {
            term: (key.term, value.term),
            score: key.score,
        });
    }
    let canonical_pairs = if pairs.len() <= 1 {
        pairs
    } else {
        ScoredTerm::sort_vec(&mut pairs);
        let mut by_canonical_key = HashMap::with_capacity(pairs.len());
        for pair in pairs {
            let (key, value) = pair.term;
            by_canonical_key.insert(key, (value, pair.score));
        }
        let mut canonical_pairs: Vec<_> = by_canonical_key
            .into_iter()
            .map(|(key, (value, score))| ScoredTerm {
                term: (key, value),
                score,
            })
            .collect();
        ScoredTerm::sort_vec(&mut canonical_pairs);
        canonical_pairs
    };

    let remainder_score = remainder_score(&emap.remainder);
    let (pair_terms, pair_scores) = split_scored_terms(canonical_pairs);
    let kvs = pair_terms
        .into_iter()
        .map(|(key, value)| crate::rhoapi::KeyValuePair {
            key: Some(key),
            value: Some(value),
        })
        .collect();

    construct_expr(
        ExprInstance::EMapBody(crate::rhoapi::EMap {
            kvs,
            locally_free: emap.locally_free.clone(),
            connective_used: emap.connective_used,
            remainder: emap.remainder.clone(),
        }),
        semantic_score(|algebra| {
            semantics::ordered_collection_score(
                algebra,
                Score::EMAP,
                Some(remainder_score),
                pair_scores,
                emap.connective_used,
            )
        }),
    )
}

// Collection canonicalization is part of consensus: ESet removes canonical
// duplicates after total score ordering, and EMap retains only each key's
// score while resolving canonical-key collisions deterministically. The PDA
// supplies children in source order and the permanent golden/differential
// suites pin both the rebuilt term and score tree.
/// The `ESetBody` arm.
#[inline(never)]
fn combine_eset(eset: &crate::rhoapi::ESet, mut kids: Vec<ScoredTerm<Par>>) -> ScoredTerm<Expr> {
    let canonical = if kids.len() <= 1 {
        kids
    } else {
        ScoredTerm::sort_vec(&mut kids);
        let mut unique = HashMap::with_capacity(kids.len());
        for child in kids {
            unique.entry(child.term).or_insert(child.score);
        }
        let mut canonical: Vec<_> = unique
            .into_iter()
            .map(|(term, score)| ScoredTerm { term, score })
            .collect();
        ScoredTerm::sort_vec(&mut canonical);
        canonical
    };

    let remainder_score = remainder_score(&eset.remainder);
    let (element_terms, element_scores) = split_scored_terms(canonical);

    construct_expr(
        ExprInstance::ESetBody(crate::rhoapi::ESet {
            ps: element_terms,
            locally_free: eset.locally_free.clone(),
            connective_used: eset.connective_used,
            remainder: eset.remainder.clone(),
        }),
        semantic_score(|algebra| {
            semantics::ordered_collection_score(
                algebra,
                Score::ESET,
                Some(remainder_score),
                element_scores,
                eset.connective_used,
            )
        }),
    )
}

/// The `EPathmapBody` arm.
#[inline(never)]
fn combine_epathmap(pathmap: &EPathMap, kids: Vec<ScoredTerm<Par>>) -> ScoredTerm<Expr> {
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
    let (canonical, entry_scores) = canonicalize_scored_pathmap(pathmap, kids, true);
    let remainder_score = remainder_score(&canonical.remainder);
    let connective_used = canonical.connective_used;

    construct_expr(
        ExprInstance::EPathmapBody(canonical),
        semantic_score(|algebra| {
            semantics::ordered_collection_score(
                algebra,
                Score::EPATHMAP,
                Some(remainder_score),
                entry_scores,
                connective_used,
            )
        }),
    )
}
