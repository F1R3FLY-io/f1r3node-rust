//! # Leg-2 Stage C-2: the explicit-worklist machine for the sorter
//!
//! Eleven `Sortable` implementations (`Par`, `Send`, `Receive`, `ReceiveBind`,
//! `New`, `Match`, `MatchCase`, `If`, `Bundle`, `Connective`, `Expr`) form one
//! mutually recursive post-order fold whose call depth is Θ(term nesting
//! depth). Measured at **78,438 bytes of native stack per bracket level** in
//! debug and **6,589** in release, that is the binding constraint on the reduce
//! path once Stage B has converted substitution: `SubstituteTrait<Par>::
//! substitute` is `substitute_no_sort` followed by exactly one
//! `ParSortMatcher::sort_match`, and after Stage B the sorted entry point's
//! per-level cost was *the sorter's constant to within 0.01%*.
//!
//! The same constant is what `Compiler::source_to_adt` (debug),
//! `SortedParHashSet::insert` and `SortedParMap::insert` all reduce to.
//!
//! This module replaces the call stack with an explicit heap worklist. Native
//! stack becomes `O(1)` in both nesting depth and sibling width; the recursion
//! lives in `work`.
//!
//! ## ★ This is an UNWEIGHTED machine, and deliberately so
//!
//! The sorter looks like it might carry a semiring weight — it computes a
//! `Tree<ScoreAtom>` alongside the term — but it does not, on four independent
//! counts, each checked against the code:
//!
//! | semiring obligation | why it fails here |
//! |---|---|
//! | a `⊕` to merge parallel runs | the traversal is **deterministic**: one input tree, one derivation. There are no parallel runs to merge. |
//! | `⊗` associative, with an identity | the candidate is `Tree::create_node_from_i32`, which is *n-ary* and *non-associative* — `Node([Node([x])]) ≠ Node([x])` — and has no identity element. |
//! | weights may not drive control | the order **does** drive control, through `ScoredTerm::sort_vec`: it decides sibling order, which *is* the output. |
//! | the weight determines the output | it does not. The score does not determine the term: the sorter is a normalizer and is not injective — `Par{exprs:[GInt 1, GInt 2]}` and `Par{exprs:[GInt 2, GInt 1]}` are unequal inputs with equal scores *and* equal canonical terms. |
//!
//! So this is a plain pushdown machine over a finite input alphabet, and the
//! score is simply one of the two things each `Combine` builds.
//!
//! ## ★ This decides the CANONICAL FORM, and the canonical form is signed
//!
//! `rholang/src/rust/interpreter/accounting/cost_accounting/sig.rs` computes
//! `ParSortMatcher::sort_match(&par).term.encode_to_vec()` — the bytes that get
//! signed. A one-element reordering is a **consensus fork**, not a performance
//! regression. Four things stand behind the claim that nothing moved:
//!
//! 1. `models/tests/sorter_canonical_golden.rs` — the encoded term and the
//!    score of a corpus covering every `ExprInstance` variant, pinned against a
//!    fixture captured from the **pre-conversion** implementation.
//! 2. [`super::sort_recursive`] — the recursive oracle twin and its
//!    differential, over the same corpus plus multi-sibling terms.
//! 3. [`SortKont::arity`] plus the **deficit invariant** in
//!    [`crate::rust::rholang::drive`] — a
//!    structural cross-check that fires on the first malformed configuration on
//!    *any* term, where a differential fires only if the corpus happens to
//!    contain the witness.
//! 4. `rholang/tests/stack_depth_gate.rs` — the depth- and width-independence
//!    gate, in both profiles.
//!
//! ⚠ **A reversal error is invisible on single-child terms.** A `Combine` that
//! pops its children in the wrong order still produces a same-*multiset*
//! result, and the transitivity half of the comparator differential will not
//! catch it either. Two things do: *result equality on multi-sibling terms*
//! (the golden corpus and `order_preserving_slots_keep_their_input_order`), and
//! the **source-index check** below, which turns "push in reverse" from a
//! convention into a failing assertion on every term any test sorts.
//!
//! ⚠★ That last sentence was FALSE until 2026-07-29 and is now true. The check
//! tagged each child with its *push position* rather than its *source
//! position*, under which every push order yields the contiguous ascending run
//! the assertion looks for — so it passed on a deliberately reversed run while
//! the other three gates all caught it. `push_reversed` now tags by source
//! position (byte-neutral on correct code, since reverse iteration assigns the
//! same numbers the old descending counter did), and the reversed-run probe
//! makes it fire. See [`assert_children_are_in_source_order`] for the
//! per-guard before/after table.
//!
//! ⚠ It would have been tempting to make the order robust instead, by sorting
//! on `(score, sibling_index)`. That is **rejected as an ordering change**:
//! `ScoredTerm::sort_vec` is `pub` and reaches `SortedParHashSet` /
//! `SortedParMap`, whose iteration order is consensus-observable. It is also
//! unnecessary — `ScoreAtom::compare` lifted through `compare_score` is a
//! *total* order on `Tree<ScoreAtom>`, so distinct score trees never compare
//! `Equal` and a stable sort by score is observationally identical to a sort by
//! `(score, index)`. The non-injectivity lives in the map *term → score*, not
//! in the order. So the idea is kept only as the driver-side assertion.
//!
//! ## Structure
//!
//! * [`SortVal`] — a produced `ScoredTerm<T>`, tagged by the entry point that
//!   produced it. It travels on the value stack inside a [`ValItem`], which
//!   also carries the value's **source index**: its position in its parent's
//!   slot list.
//! * [`NodeKind`] — one **borrowed** input node. Travels inside a
//!   [`SortNode`], which carries the source index the produced value must end
//!   up with; a [`SortKont`] travels inside an [`IxKont`] for the same reason.
//! * [`SortKont`] — the post-order continuation: the borrowed *shell* (flags,
//!   cached bitsets, counts, remainders — everything that is not a child) plus
//!   the child counts needed to slice the value stack.
//! * [`SortTraversal`] — the [`Traversal`] impl. The LIFO loop itself is
//!   [`crate::rust::rholang::drive::drive`], shared with the family's other
//!   converted members; [`sort_drive`] is the typed entry into it.
//!
//! ```text
//!   work (LIFO)                          vals (LIFO)
//!   ┌────────────────┐                   ┌──────────────────────────┐
//!   │ Par(&p)   ix=i │ ──descend──▶      │                          │
//!   ├────────────────┤                   │                          │
//!   │ Combine(K) ix=i│ ◀── pushed FIRST  │  ix=0 ix=1 … ix=n-1      │
//!   │ child   ix=0   │                   │  ▲ ascending, contiguous │
//!   │ …              │ ◀── pushed in     │  └── debug_assert-ed     │
//!   │ child   ix=n-1 │     REVERSE       │                          │
//!   └────────────────┘                   └──────────────────────────┘
//! ```
//!
//! ## Why the work items BORROW
//!
//! Sorting *reads* its input and builds a fresh output — unlike substitution,
//! which consumes. Borrowing means the worklist performs **zero** deep copies
//! on the structural spine: pushing a child is a reference move, never a
//! `<Par as Clone>::clone` (which is itself Θ(depth) at 15,914 B/level, debug,
//! and would have re-introduced the very class this conversion removes).
//!
//! ## ⚠ Three arms run their own bounded drive
//!
//! `ESetBody`, `EMapBody` and `EPathmapBody` re-enter `ParSortMatcher::
//! sort_match` on **owned intermediates** (a deduplicated `HashSet`, a
//! canonicalised trie order) rather than on sub-terms of the input, so they
//! cannot be borrowed onto this worklist. They are kept verbatim in
//! [`super::sort_combine`], each re-entry starting a fresh bounded drive.
//!
//! That residual is a **tripwire, not a conversion target**, and the
//! justification is measured rather than asserted: those arms sort each element
//! three times, so a chain of `n` nested sets costs `3^n` sorts — depth 20 is
//! 3.5 × 10⁹ sorts and did not terminate in either profile. Deep set nesting is
//! infeasible in *time* long before the stack residual could bite. What the
//! tripwire must guarantee is that it does not **regress**: see
//! `sort_nested_set` / `sort_nested_map` in `rholang/tests/stack_depth_gate.rs`,
//! pinned at the measured pre-conversion baselines (79,053 / 82,534 B/level,
//! debug).
//!
//! Full analysis, measured constants and proof standard:
//! `docs/design/audits/theta-depth-traversals-2026-07-26.md`.

use super::score_tree::ScoredTerm;
use super::sort_combine::{
    combine_bind, combine_bundle, combine_case, combine_connective, combine_expr, combine_if,
    combine_match, combine_new, combine_par, combine_receive, combine_send, connective_child_pars,
    empty_par, expr_child_pars, sort_unforgeable, ParParts,
};
use crate::rhoapi::{
    Bundle, Connective, Expr, GUnforgeable, If, Match, MatchCase, New, Par, Receive, ReceiveBind,
    Send,
};
use crate::rust::rholang::drive::{drive, Outcome, Step, Traversal};

// ===========================================================================
// values, work, continuations
// ===========================================================================

/// A produced `ScoredTerm`, tagged by the entry point that produced it.
pub(crate) enum SortVal {
    Par(ScoredTerm<Par>),
    Expr(ScoredTerm<Expr>),
    Send(ScoredTerm<Send>),
    Receive(ScoredTerm<Receive>),
    Bind(ScoredTerm<ReceiveBind>),
    New(ScoredTerm<New>),
    Match(ScoredTerm<Match>),
    Case(ScoredTerm<MatchCase>),
    If(ScoredTerm<If>),
    Bundle(ScoredTerm<Bundle>),
    Connective(ScoredTerm<Connective>),
    Unforgeable(ScoredTerm<GUnforgeable>),
}

/// A value plus its **source index** — its position in its parent's slot list.
///
/// The index exists so that "push children in reverse so they pop in source
/// order" is *checked* rather than merely intended; see
/// [`assert_children_are_in_source_order`].
pub(crate) struct ValItem {
    /// Position among the parent's children, `0`-based, in slot order.
    pub idx: u32,
    pub val: SortVal,
}

/// An input node. Every AST reference borrows the input term (`'t`).
///
/// `Copy`, as [`Traversal::Node`] requires: pushing a child is a reference move
/// and never a `<Par as Clone>::clone` (itself Θ(depth) at 15,914 B/level,
/// debug, which would have re-introduced the class this conversion removes).
#[derive(Clone, Copy)]
pub(crate) enum NodeKind<'t> {
    Par(&'t Par),
    Expr(&'t Expr),
    Send(&'t Send),
    Receive(&'t Receive),
    Bind(&'t ReceiveBind),
    New(&'t New),
    Match(&'t Match),
    Case(&'t MatchCase),
    If(&'t If),
    Bundle(&'t Bundle),
    Connective(&'t Connective),
    /// A leaf: no sub-`Par`, scored in place. (`Var` is a leaf too, but it is
    /// never a *child slot* of anything this machine descends into — the only
    /// `Var` positions are `EVar::v` and the two remainders, all scored in
    /// place inside `sort_combine` — so it has no node variant.)
    Unforgeable(&'t GUnforgeable),
}

/// A node plus the **source index** the value it produces must carry.
///
/// ★ This wrapper is why [`Traversal::Node`] is an *associated type* and not a
/// fixed shape: the sorter needs one `u32` of per-item metadata that no other
/// instance does, and it rides inside the node rather than widening the
/// driver's [`Step`] for everybody.
#[derive(Clone, Copy)]
pub(crate) struct SortNode<'t> {
    pub idx: u32,
    pub kind: NodeKind<'t>,
}

/// A continuation plus the source index its produced value must carry — the
/// [`SortNode`] wrapper's counterpart on the `Combine` side.
pub(crate) struct IxKont<'t> {
    pub idx: u32,
    pub kont: SortKont<'t>,
}

/// Post-order continuation: the borrowed shell plus the child counts needed to
/// slice the value stack. Never a child value — those live on `vals`.
pub(crate) enum SortKont<'t> {
    ParK {
        par: &'t Par,
        n_sends: usize,
        n_receives: usize,
        n_exprs: usize,
        n_news: usize,
        n_matches: usize,
        n_bundles: usize,
        n_connectives: usize,
        n_unforgeables: usize,
        n_conditionals: usize,
    },
    SendK {
        send: &'t Send,
        n_data: usize,
    },
    BindK {
        bind: &'t ReceiveBind,
        n_patterns: usize,
    },
    ReceiveK {
        recv: &'t Receive,
        n_binds: usize,
    },
    NewK {
        new: &'t New,
        n_injections: usize,
    },
    CaseK {
        case: &'t MatchCase,
    },
    MatchK {
        m: &'t Match,
        n_cases: usize,
    },
    IfK {
        cond: &'t If,
    },
    BundleK {
        bundle: &'t Bundle,
    },
    ConnK {
        conn: &'t Connective,
        n: usize,
    },
    ExprK {
        expr: &'t Expr,
        n: usize,
    },
}

impl SortKont<'_> {
    /// The number of values this continuation pops.
    ///
    /// ★ **This deliberately duplicates the pop counts in the per-arm combine
    /// functions.** It is not a convenience accessor: it is an *independent
    /// statement* of each continuation's arity, and the whole point is to
    /// cross-check it against what the arms actually pop. Three checks consume
    /// it — [`assert_children_are_in_source_order`], the pushed-child count in
    /// [`sort_drive`], and the deficit invariant — and any of them fires if
    /// this and the arms disagree.
    ///
    /// The `match` is exhaustive with no `_` arm, so a continuation added
    /// without an arity is a compile error.
    fn arity(&self) -> usize {
        match self {
            SortKont::ParK {
                n_sends,
                n_receives,
                n_exprs,
                n_news,
                n_matches,
                n_bundles,
                n_connectives,
                n_unforgeables,
                n_conditionals,
                ..
            } => {
                n_sends
                    + n_receives
                    + n_exprs
                    + n_news
                    + n_matches
                    + n_bundles
                    + n_connectives
                    + n_unforgeables
                    + n_conditionals
            }
            // `chan` + `data`
            SortKont::SendK { n_data, .. } => 1 + n_data,
            // `patterns` + `source`
            SortKont::BindK { n_patterns, .. } => n_patterns + 1,
            // `binds` + `body` + `condition`
            SortKont::ReceiveK { n_binds, .. } => n_binds + 2,
            // `p` + one per injection value
            SortKont::NewK { n_injections, .. } => 1 + n_injections,
            // `pattern` + `source` + `guard`
            SortKont::CaseK { .. } => 3,
            // `target` + `cases`
            SortKont::MatchK { n_cases, .. } => 1 + n_cases,
            // `condition` + `if_true` + `if_false`
            SortKont::IfK { .. } => 3,
            // `body`
            SortKont::BundleK { .. } => 1,
            SortKont::ConnK { n, .. } => *n,
            SortKont::ExprK { n, .. } => *n,
        }
    }
}

// ---------------------------------------------------------------------------
// value-stack pop helpers
//
// Type discipline: the producing `NodeKind` guarantees the variant, so a
// mismatch is a driver bug and never an input error. That already catches
// popping the wrong CATEGORY; the source-index checks below catch popping the
// right category in the wrong ORDER.
// ---------------------------------------------------------------------------

macro_rules! sort_pop_ix {
    ($name:ident, $variant:ident, $ty:ty) => {
        #[inline]
        fn $name(vals: &mut Vec<ValItem>) -> (u32, ScoredTerm<$ty>) {
            match vals.pop() {
                Some(ValItem {
                    idx,
                    val: SortVal::$variant(v),
                }) => (idx, v),
                _ => unreachable!(concat!(
                    "sort_drive: expected ",
                    stringify!($variant),
                    " on the value stack"
                )),
            }
        }
    };
}

sort_pop_ix!(pop_par_ix, Par, Par);
sort_pop_ix!(pop_expr_ix, Expr, Expr);
sort_pop_ix!(pop_send_ix, Send, Send);
sort_pop_ix!(pop_receive_ix, Receive, Receive);
sort_pop_ix!(pop_bind_ix, Bind, ReceiveBind);
sort_pop_ix!(pop_new_ix, New, New);
sort_pop_ix!(pop_match_ix, Match, Match);
sort_pop_ix!(pop_case_ix, Case, MatchCase);
sort_pop_ix!(pop_if_ix, If, If);
sort_pop_ix!(pop_bundle_ix, Bundle, Bundle);
sort_pop_ix!(pop_connective_ix, Connective, Connective);
sort_pop_ix!(pop_unforgeable_ix, Unforgeable, GUnforgeable);

/// Pop one `Par` value, discarding its index. The whole child run has already
/// been checked by [`assert_children_are_in_source_order`].
#[inline]
fn pop_par(vals: &mut Vec<ValItem>) -> ScoredTerm<Par> {
    pop_par_ix(vals).1
}

macro_rules! sort_pop_n {
    ($name:ident, $one:ident, $ty:ty) => {
        /// Pop `n` values, returning them in FORWARD (push) order.
        ///
        /// ⚠ Asserts the popped source indices descend by exactly one, which is
        /// the same statement as "the returned vector is in slot order". A
        /// `descend_*` that forgot to push in reverse trips here on the first
        /// multi-child node rather than silently producing a same-multiset
        /// result with a different canonical form.
        #[inline]
        fn $name(vals: &mut Vec<ValItem>, n: usize) -> Vec<ScoredTerm<$ty>> {
            let mut out = Vec::with_capacity(n);
            let mut previous: Option<u32> = None;
            for _ in 0..n {
                let (idx, v) = $one(vals);
                if let Some(p) = previous {
                    debug_assert_eq!(
                        idx + 1,
                        p,
                        concat!(
                            "sort_drive: ",
                            stringify!($name),
                            " popped children out of source order — a `descend_*` did not \
                             push in reverse. That changes SIBLING ORDER, which is the \
                             canonical form, which is what cost_accounting/sig.rs signs."
                        )
                    );
                }
                previous = Some(idx);
                out.push(v);
            }
            out.reverse();
            out
        }
    };
}

sort_pop_n!(pop_n_par, pop_par_ix, Par);
sort_pop_n!(pop_n_expr, pop_expr_ix, Expr);
sort_pop_n!(pop_n_send, pop_send_ix, Send);
sort_pop_n!(pop_n_receive, pop_receive_ix, Receive);
sort_pop_n!(pop_n_bind, pop_bind_ix, ReceiveBind);
sort_pop_n!(pop_n_new, pop_new_ix, New);
sort_pop_n!(pop_n_match, pop_match_ix, Match);
sort_pop_n!(pop_n_case, pop_case_ix, MatchCase);
sort_pop_n!(pop_n_if, pop_if_ix, If);
sort_pop_n!(pop_n_bundle, pop_bundle_ix, Bundle);
sort_pop_n!(pop_n_connective, pop_connective_ix, Connective);
sort_pop_n!(pop_n_unforgeable, pop_unforgeable_ix, GUnforgeable);

/// The children a `Combine` is about to consume must sit on the value stack as
/// a contiguous ascending run `0, 1, …, arity-1`.
///
/// ★ This is the assertion that makes "push in reverse" checkable, and it earns
/// that description only because [`push_reversed`] tags each child with its
/// **position in the source slice**.
///
/// ⚠★ CORRECTED 2026-07-29, and the correction was MEASURED. `push_reversed`
/// previously derived `idx` from a **descending push counter**, so the index
/// recorded *where the child was pushed*, not *where it came from*. Under that
/// scheme every push order produces a contiguous ascending run on the value
/// stack, and the assertion below is satisfied by all of them — so it did NOT
/// make "push in reverse" checkable, in spite of saying so. The probe:
/// `push_reversed` was changed to iterate forward and the suite re-run.
///
/// | guard | reversed-run defect, push-counter `idx` | reversed-run defect, source-position `idx` |
/// |---|---|---|
/// | this assertion | **PASSED** (vacuous) | fires |
/// | `order_preserving_slots_keep_their_input_order` | fires | fires |
/// | `the_worklist_driver_agrees_with_the_recursive_oracle_*` | fires | fires |
/// | `sorter_canonical_golden` | fires | fires |
///
/// The canonical form was never unguarded — three independent gates caught the
/// defect. But this one was carrying a claim it could not support, which is the
/// thing that makes a guard rot: the next person reads the claim, not the
/// arithmetic. `idx` is now the source position, and the row above is why.
///
/// It also cross-checks [`SortKont::arity`] against the layout the `descend_*`
/// handler actually produced: a miscounted slot list shifts the run and trips
/// here. That half was always real.
#[inline]
fn assert_children_are_in_source_order(vals: &[ValItem], arity: usize) {
    debug_assert!(
        vals.len() >= arity,
        "sort_drive: a Combine of arity {} found only {} values on the stack",
        arity,
        vals.len()
    );
    let base = vals.len() - arity;
    for (offset, item) in vals[base..].iter().enumerate() {
        debug_assert_eq!(
            item.idx as usize, offset,
            "sort_drive: child values are not a contiguous ascending source-index run \
             (arity {}, offset {}, found index {}). Either a `descend_*` pushed children \
             in the wrong order, or `SortKont::arity` disagrees with the slot list.",
            arity, offset, item.idx
        );
    }
}

/// Push a borrowed slice so that its elements are POPPED in source order,
/// tagging each with its source index.
///
/// `next` counts **down** from the node's total child count, so the item pushed
/// first (and popped last) carries the highest index.
#[inline]
fn push_reversed<'t, T, F>(
    work: &mut Vec<Step<'t, SortTraversal>>,
    next: &mut u32,
    items: &'t [T],
    mut f: F,
) where
    F: FnMut(&'t T) -> NodeKind<'t>,
{
    // ★ `idx` is the item's position in `items`, NOT the order it was pushed
    // in. On correct code the two coincide exactly — reverse iteration assigns
    // `base + len - 1` down to `base`, which is what the old descending counter
    // produced — so this is byte-neutral. On a REVERSED run they differ, and
    // that difference is the whole point: see
    // [`assert_children_are_in_source_order`] for the measurement showing that
    // a push-position `idx` made that assertion vacuous.
    let base = *next - items.len() as u32;
    for (offset, item) in items.iter().enumerate().rev() {
        work.push(Step::Descend(SortNode {
            idx: base + offset as u32,
            kind: f(item),
        }));
    }
    *next = base;
}

/// Push one child, tagging it with the next (descending) source index.
#[inline]
fn push_one<'t>(work: &mut Vec<Step<'t, SortTraversal>>, next: &mut u32, kind: NodeKind<'t>) {
    *next -= 1;
    work.push(Step::Descend(SortNode { idx: *next, kind }));
}

// ===========================================================================
// descent — pushes `Combine` first, then children in REVERSE
// ===========================================================================

fn descend_par<'t>(par: &'t Par, idx: u32, work: &mut Vec<Step<'t, SortTraversal>>) {
    let kont = SortKont::ParK {
        par,
        n_sends: par.sends.len(),
        n_receives: par.receives.len(),
        n_exprs: par.exprs.len(),
        n_news: par.news.len(),
        n_matches: par.matches.len(),
        n_bundles: par.bundles.len(),
        n_connectives: par.connectives.len(),
        n_unforgeables: par.unforgeables.len(),
        n_conditionals: par.conditionals.len(),
    };
    let mut next = kont.arity() as u32;
    work.push(Step::Combine(IxKont { idx, kont }));
    // Visit order is the pre-conversion matcher's: sends, receives, exprs,
    // news, matches, bundles, connectives, unforgeables, conditionals. Pushed
    // in reverse so LIFO pops them in that order.
    push_reversed(work, &mut next, &par.conditionals, NodeKind::If);
    push_reversed(work, &mut next, &par.unforgeables, NodeKind::Unforgeable);
    push_reversed(work, &mut next, &par.connectives, NodeKind::Connective);
    push_reversed(work, &mut next, &par.bundles, NodeKind::Bundle);
    push_reversed(work, &mut next, &par.matches, NodeKind::Match);
    push_reversed(work, &mut next, &par.news, NodeKind::New);
    push_reversed(work, &mut next, &par.exprs, NodeKind::Expr);
    push_reversed(work, &mut next, &par.receives, NodeKind::Receive);
    push_reversed(work, &mut next, &par.sends, NodeKind::Send);
    debug_assert_eq!(next, 0, "descend_par: slot list and arity disagree");
}

fn descend_send<'t>(send: &'t Send, idx: u32, work: &mut Vec<Step<'t, SortTraversal>>) {
    let chan = send
        .chan
        .as_ref()
        .expect("channel field on Send was None, should be Some");
    let kont = SortKont::SendK {
        send,
        n_data: send.data.len(),
    };
    let mut next = kont.arity() as u32;
    work.push(Step::Combine(IxKont { idx, kont }));
    push_reversed(work, &mut next, &send.data, NodeKind::Par);
    push_one(work, &mut next, NodeKind::Par(chan));
    debug_assert_eq!(next, 0, "descend_send: slot list and arity disagree");
}

fn descend_bind<'t>(bind: &'t ReceiveBind, idx: u32, work: &mut Vec<Step<'t, SortTraversal>>) {
    // ⚠ The pre-conversion `sort_bind` evaluates this `expect` BEFORE it sorts
    // any pattern, so a bind with a missing source panics with this message
    // rather than with anything a pattern might raise. Evaluating it here
    // preserves that.
    let source = bind
        .source
        .as_ref()
        .expect("source field on Bind was None, should be Some");
    let kont = SortKont::BindK {
        bind,
        n_patterns: bind.patterns.len(),
    };
    let mut next = kont.arity() as u32;
    work.push(Step::Combine(IxKont { idx, kont }));
    push_one(work, &mut next, NodeKind::Par(source));
    push_reversed(work, &mut next, &bind.patterns, NodeKind::Par);
    debug_assert_eq!(next, 0, "descend_bind: slot list and arity disagree");
}

fn descend_receive<'t>(recv: &'t Receive, idx: u32, work: &mut Vec<Step<'t, SortTraversal>>) {
    let body = recv
        .body
        .as_ref()
        .expect("body field on Receive was None, should be Some");
    // The pre-conversion matcher sorted `r.condition.clone().unwrap_or_default()`.
    // Sorting the borrowed field against a shared empty `Par` is the same value
    // without the Θ(depth) `<Par as Clone>::clone`; see `sort_combine::empty_par`.
    //
    // `&'static Par` coerces to `&'t Par` here; a bare
    // `unwrap_or_else(empty_par)` would instead unify `'t` with `'static` and
    // refuse the borrow.
    let condition = match recv.condition.as_ref() {
        Some(p) => p,
        None => empty_par(),
    };
    let kont = SortKont::ReceiveK {
        recv,
        n_binds: recv.binds.len(),
    };
    let mut next = kont.arity() as u32;
    work.push(Step::Combine(IxKont { idx, kont }));
    push_one(work, &mut next, NodeKind::Par(condition));
    push_one(work, &mut next, NodeKind::Par(body));
    push_reversed(work, &mut next, &recv.binds, NodeKind::Bind);
    debug_assert_eq!(next, 0, "descend_receive: slot list and arity disagree");
}

fn descend_new<'t>(new: &'t New, idx: u32, work: &mut Vec<Step<'t, SortTraversal>>) {
    let p = new
        .p
        .as_ref()
        .expect("p field on New was None, should be Some");
    let kont = SortKont::NewK {
        new,
        n_injections: new.injections.len(),
    };
    let mut next = kont.arity() as u32;
    work.push(Step::Combine(IxKont { idx, kont }));
    // `injections` is a `BTreeMap`, so `values()` is key order — the same order
    // the pre-conversion matcher's `into_iter()` produced, and the order
    // `combine_new` re-zips against `keys()`.
    for v in new.injections.values().rev() {
        push_one(work, &mut next, NodeKind::Par(v));
    }
    push_one(work, &mut next, NodeKind::Par(p));
    debug_assert_eq!(next, 0, "descend_new: slot list and arity disagree");
}

fn descend_case<'t>(case: &'t MatchCase, idx: u32, work: &mut Vec<Step<'t, SortTraversal>>) {
    let pattern = case
        .pattern
        .as_ref()
        .expect("pattern field on MatchCase was None, should be Some");
    let source = case
        .source
        .as_ref()
        .expect("source field on MatchCase was None, should be Some");
    let guard = match case.guard.as_ref() {
        Some(p) => p,
        None => empty_par(),
    };
    let kont = SortKont::CaseK { case };
    let mut next = kont.arity() as u32;
    work.push(Step::Combine(IxKont { idx, kont }));
    push_one(work, &mut next, NodeKind::Par(guard));
    push_one(work, &mut next, NodeKind::Par(source));
    push_one(work, &mut next, NodeKind::Par(pattern));
    debug_assert_eq!(next, 0, "descend_case: slot list and arity disagree");
}

fn descend_match<'t>(m: &'t Match, idx: u32, work: &mut Vec<Step<'t, SortTraversal>>) {
    let target = m
        .target
        .as_ref()
        .expect("target field on Match was None, should be Some");
    let kont = SortKont::MatchK {
        m,
        n_cases: m.cases.len(),
    };
    let mut next = kont.arity() as u32;
    work.push(Step::Combine(IxKont { idx, kont }));
    push_reversed(work, &mut next, &m.cases, NodeKind::Case);
    push_one(work, &mut next, NodeKind::Par(target));
    debug_assert_eq!(next, 0, "descend_match: slot list and arity disagree");
}

fn descend_if<'t>(cond: &'t If, idx: u32, work: &mut Vec<Step<'t, SortTraversal>>) {
    let condition = cond
        .condition
        .as_ref()
        .expect("condition field on If was None, should be Some");
    let if_true = cond
        .if_true
        .as_ref()
        .expect("if_true field on If was None, should be Some");
    let if_false = cond
        .if_false
        .as_ref()
        .expect("if_false field on If was None, should be Some");
    let kont = SortKont::IfK { cond };
    let mut next = kont.arity() as u32;
    work.push(Step::Combine(IxKont { idx, kont }));
    push_one(work, &mut next, NodeKind::Par(if_false));
    push_one(work, &mut next, NodeKind::Par(if_true));
    push_one(work, &mut next, NodeKind::Par(condition));
    debug_assert_eq!(next, 0, "descend_if: slot list and arity disagree");
}

fn descend_bundle<'t>(bundle: &'t Bundle, idx: u32, work: &mut Vec<Step<'t, SortTraversal>>) {
    let body = bundle
        .body
        .as_ref()
        .expect("body was None, should be Some(Par)");
    let kont = SortKont::BundleK { bundle };
    let mut next = kont.arity() as u32;
    work.push(Step::Combine(IxKont { idx, kont }));
    push_one(work, &mut next, NodeKind::Par(body));
    debug_assert_eq!(next, 0, "descend_bundle: slot list and arity disagree");
}

fn descend_connective<'t>(conn: &'t Connective, idx: u32, work: &mut Vec<Step<'t, SortTraversal>>) {
    let mut kids: Vec<&'t Par> = Vec::new();
    connective_child_pars(conn, &mut kids);
    let kont = SortKont::ConnK {
        conn,
        n: kids.len(),
    };
    let mut next = kont.arity() as u32;
    work.push(Step::Combine(IxKont { idx, kont }));
    for p in kids.into_iter().rev() {
        push_one(work, &mut next, NodeKind::Par(p));
    }
    debug_assert_eq!(next, 0, "descend_connective: slot list and arity disagree");
}

fn descend_expr<'t>(expr: &'t Expr, idx: u32, work: &mut Vec<Step<'t, SortTraversal>>) {
    let mut kids: Vec<&'t Par> = Vec::new();
    expr_child_pars(expr, &mut kids);
    let kont = SortKont::ExprK {
        expr,
        n: kids.len(),
    };
    let mut next = kont.arity() as u32;
    work.push(Step::Combine(IxKont { idx, kont }));
    for p in kids.into_iter().rev() {
        push_one(work, &mut next, NodeKind::Par(p));
    }
    debug_assert_eq!(next, 0, "descend_expr: slot list and arity disagree");
}

// ===========================================================================
// the driver — an instance of `models::rust::rholang::drive`
// ===========================================================================
//
// ★ STAGE F-2. The LIFO loop that used to live here is now
// `crate::rust::rholang::drive::drive`, shared with `rho-pure-eval`'s
// `eval_with` (stage F-1) and, from stage F-3, with the codec. What moved is
// only the loop, the two stacks and the invariants; every `descend_*` above and
// every `combine_*_k` below is byte-for-byte the code that produced the
// canonical form before the move.
//
// Three of the old loop's own checks are worth accounting for individually,
// because "it moved" is not the same claim for each:
//
// | old check | now |
// |---|---|
// | the deficit invariant `V + D + C - A == 1` | `drive.rs` Invariant 2, verbatim — the derivation moved with it |
// | "a `descend_*` must push its `Combine` first" + `pushed - 1 == arity` | `drive.rs` Invariant 1, GENERALIZED to a right-to-left availability scan. The sorter's regions are all `[Combine, children…]`, so it accepts exactly what the old rule accepted |
// | `debug_assert_eq!(before, work.len(), "a Combine must not push work")` | ★ ENFORCED BY THE TYPE SYSTEM. `Traversal::combine` is not given the work stack, so a combine that pushed work no longer compiles |
//
// ⚠ `assert_children_are_in_source_order` did NOT move into the driver, and
// must not: it is a statement about the *sorter's* value layout (a contiguous
// ascending source-index run), not about any traversal's. It runs at the head of
// `combine`, which is the same point in the same order as the bespoke loop ran
// it — before any pop.

/// The sorter as a [`Traversal`]. Zero-sized: sorting reads no ambient state.
pub(crate) struct SortTraversal;

/// Sorting cannot fail. `Infallible` states that in the type system rather than
/// in a comment, and it is what lets [`sort_drive`] have no panic path at all.
type Never = std::convert::Infallible;

impl Traversal for SortTraversal {
    type Node<'t> = SortNode<'t>;
    type Val = ValItem;
    type Kont<'t> = IxKont<'t>;
    /// Nothing is threaded: the score and the term are both built on the value
    /// stack, and the input is only read.
    type State = ();
    type Err = Never;

    fn descend<'t>(
        &mut self,
        _state: &mut (),
        node: SortNode<'t>,
        work: &mut Vec<Step<'t, Self>>,
        vals: &mut Vec<ValItem>,
    ) -> Result<(), Never> {
        let SortNode { idx, kind } = node;
        match kind {
            NodeKind::Par(p) => descend_par(p, idx, work),
            NodeKind::Expr(e) => descend_expr(e, idx, work),
            NodeKind::Send(s) => descend_send(s, idx, work),
            NodeKind::Receive(r) => descend_receive(r, idx, work),
            NodeKind::Bind(b) => descend_bind(b, idx, work),
            NodeKind::New(n) => descend_new(n, idx, work),
            NodeKind::Match(m) => descend_match(m, idx, work),
            NodeKind::Case(c) => descend_case(c, idx, work),
            NodeKind::If(i) => descend_if(i, idx, work),
            NodeKind::Bundle(b) => descend_bundle(b, idx, work),
            NodeKind::Connective(c) => descend_connective(c, idx, work),
            // The one leaf: no sub-`Par`, scored in place.
            NodeKind::Unforgeable(u) => vals.push(ValItem {
                idx,
                val: SortVal::Unforgeable(sort_unforgeable(u)),
            }),
        }
        Ok(())
    }

    fn combine<'t>(
        &mut self,
        _state: &mut (),
        kont: IxKont<'t>,
        vals: &mut Vec<ValItem>,
    ) -> Result<Outcome<ValItem>, Never> {
        let IxKont { idx, kont } = kont;
        // ⚠ BEFORE any pop, exactly where the bespoke loop ran it. This is the
        // assertion that turns "push in reverse" from a convention into a
        // failure on every term any test sorts.
        assert_children_are_in_source_order(vals, kont.arity());
        let val = run_combine(kont, vals);
        // ⚠ NO EARLY EXIT, and here that is a CONSENSUS statement rather than a
        // performance one. The sorter is a normalizer: every continuation's
        // result is a child of the next, so no combine can already hold the
        // answer — and a `Done` that returned early would emit a term that is
        // not the canonical form, which is a fork rather than a slow path.
        // `Outcome::Done` belongs to the comparison traversals.
        Ok(Outcome::Value(ValItem { idx, val }))
    }

    /// ★ Delegates to [`SortKont::arity`], which deliberately duplicates the
    /// pop counts in the per-arm combines so the driver's invariants can
    /// cross-check them. Exhaustive there, with no `_` arm.
    fn arity(kont: &IxKont<'_>) -> usize {
        kont.kont.arity()
    }
}

/// Sort one node, starting a fresh bounded drive.
///
/// Native stack is `O(1)` in both nesting depth and sibling width; the
/// recursion lives in the driver's heap work stack.
pub(crate) fn sort_drive(root: NodeKind<'_>) -> SortVal {
    let mut visitor = SortTraversal;
    let mut state = ();
    match drive(
        &mut visitor,
        &mut state,
        Step::Descend(SortNode { idx: 0, kind: root }),
    ) {
        Ok(item) => item.val,
        // `Never` is uninhabited, so this arm is unreachable BY TYPE. Matching
        // on it is how that is said to the compiler; an `.expect(…)` would say
        // it only to a reader and would leave a panic path in the binary.
        Err(never) => match never {},
    }
}

// ===========================================================================
// post-order reassembly — one `#[inline(never)]` function per continuation
// ===========================================================================
//
// ★ Same rationale as the per-arm split of `combine_expr` (see
// `sort_combine.rs`): at `-O0` rustc does not overlap the stack slots of
// mutually exclusive `match` arms, and the draft's single `run_combine` was
// measured by `gdb` at **22,768 bytes**. That frame is harmless on the flat
// spine but sits on the re-entrant chain of the three self-contained `Expr`
// arms, once per nesting level. Splitting bounds each arm's frame by its own
// locals *by construction*, and it is pure code motion.

/// Post-order reassembly — the **dispatcher**.
///
/// ⚠ Children were produced in push order, so they are popped in REVERSE push
/// order; `pop_n_*` restores forward order and asserts the source indices
/// descend by one while doing it.
#[inline]
fn run_combine(k: SortKont<'_>, vals: &mut Vec<ValItem>) -> SortVal {
    match k {
        SortKont::ParK {
            par,
            n_sends,
            n_receives,
            n_exprs,
            n_news,
            n_matches,
            n_bundles,
            n_connectives,
            n_unforgeables,
            n_conditionals,
        } => combine_par_k(
            par,
            [
                n_sends,
                n_receives,
                n_exprs,
                n_news,
                n_matches,
                n_bundles,
                n_connectives,
                n_unforgeables,
                n_conditionals,
            ],
            vals,
        ),
        SortKont::SendK { send, n_data } => combine_send_k(send, n_data, vals),
        SortKont::BindK { bind, n_patterns } => combine_bind_k(bind, n_patterns, vals),
        SortKont::ReceiveK { recv, n_binds } => combine_receive_k(recv, n_binds, vals),
        SortKont::NewK { new, n_injections } => combine_new_k(new, n_injections, vals),
        SortKont::CaseK { case } => combine_case_k(case, vals),
        SortKont::MatchK { m, n_cases } => combine_match_k(m, n_cases, vals),
        SortKont::IfK { cond } => combine_if_k(cond, vals),
        SortKont::BundleK { bundle } => combine_bundle_k(bundle, vals),
        SortKont::ConnK { conn, n } => combine_conn_k(conn, n, vals),
        SortKont::ExprK { expr, n } => combine_expr_k(expr, n, vals),
    }
}

/// The nine category counts, in `descend_par`'s visit order.
const N_SENDS: usize = 0;
const N_RECEIVES: usize = 1;
const N_EXPRS: usize = 2;
const N_NEWS: usize = 3;
const N_MATCHES: usize = 4;
const N_BUNDLES: usize = 5;
const N_CONNECTIVES: usize = 6;
const N_UNFORGEABLES: usize = 7;
const N_CONDITIONALS: usize = 8;

#[inline(never)]
fn combine_par_k(par: &Par, counts: [usize; 9], vals: &mut Vec<ValItem>) -> SortVal {
    // Popped in the REVERSE of `descend_par`'s visit order.
    let conditionals = pop_n_if(vals, counts[N_CONDITIONALS]);
    let unforgeables = pop_n_unforgeable(vals, counts[N_UNFORGEABLES]);
    let connectives = pop_n_connective(vals, counts[N_CONNECTIVES]);
    let bundles = pop_n_bundle(vals, counts[N_BUNDLES]);
    let matches = pop_n_match(vals, counts[N_MATCHES]);
    let news = pop_n_new(vals, counts[N_NEWS]);
    let exprs = pop_n_expr(vals, counts[N_EXPRS]);
    let receives = pop_n_receive(vals, counts[N_RECEIVES]);
    let sends = pop_n_send(vals, counts[N_SENDS]);
    SortVal::Par(combine_par(
        par,
        ParParts {
            sends,
            receives,
            exprs,
            news,
            matches,
            bundles,
            connectives,
            unforgeables,
            conditionals,
        },
    ))
}

#[inline(never)]
fn combine_send_k(send: &Send, n_data: usize, vals: &mut Vec<ValItem>) -> SortVal {
    let data = pop_n_par(vals, n_data);
    let chan = pop_par(vals);
    SortVal::Send(combine_send(send, chan, data))
}

#[inline(never)]
fn combine_bind_k(bind: &ReceiveBind, n_patterns: usize, vals: &mut Vec<ValItem>) -> SortVal {
    let channel = pop_par(vals);
    let patterns = pop_n_par(vals, n_patterns);
    SortVal::Bind(combine_bind(bind, patterns, channel))
}

#[inline(never)]
fn combine_receive_k(recv: &Receive, n_binds: usize, vals: &mut Vec<ValItem>) -> SortVal {
    let condition = pop_par(vals);
    let body = pop_par(vals);
    let binds = pop_n_bind(vals, n_binds);
    SortVal::Receive(combine_receive(recv, binds, body, condition))
}

#[inline(never)]
fn combine_new_k(new: &New, n_injections: usize, vals: &mut Vec<ValItem>) -> SortVal {
    let injections = pop_n_par(vals, n_injections);
    let p = pop_par(vals);
    SortVal::New(combine_new(new, p, injections))
}

#[inline(never)]
fn combine_case_k(case: &MatchCase, vals: &mut Vec<ValItem>) -> SortVal {
    let guard = pop_par(vals);
    let source = pop_par(vals);
    let pattern = pop_par(vals);
    SortVal::Case(combine_case(case, pattern, source, guard))
}

#[inline(never)]
fn combine_match_k(m: &Match, n_cases: usize, vals: &mut Vec<ValItem>) -> SortVal {
    let cases = pop_n_case(vals, n_cases);
    let target = pop_par(vals);
    SortVal::Match(combine_match(m, target, cases))
}

#[inline(never)]
fn combine_if_k(cond: &If, vals: &mut Vec<ValItem>) -> SortVal {
    let if_false = pop_par(vals);
    let if_true = pop_par(vals);
    let condition = pop_par(vals);
    SortVal::If(combine_if(cond, condition, if_true, if_false))
}

#[inline(never)]
fn combine_bundle_k(bundle: &Bundle, vals: &mut Vec<ValItem>) -> SortVal {
    let body = pop_par(vals);
    SortVal::Bundle(combine_bundle(bundle, body))
}

#[inline(never)]
fn combine_conn_k(conn: &Connective, n: usize, vals: &mut Vec<ValItem>) -> SortVal {
    let kids = pop_n_par(vals, n);
    SortVal::Connective(combine_connective(conn, kids))
}

#[inline(never)]
fn combine_expr_k(expr: &Expr, n: usize, vals: &mut Vec<ValItem>) -> SortVal {
    let kids = pop_n_par(vals, n);
    SortVal::Expr(combine_expr(expr, kids))
}

// ===========================================================================
// the typed entry points
// ===========================================================================

macro_rules! sort_entry {
    ($name:ident, $work:ident, $variant:ident, $ty:ty) => {
        /// Sort one node, starting a fresh bounded drive.
        pub(crate) fn $name(term: &$ty) -> ScoredTerm<$ty> {
            match sort_drive(NodeKind::$work(term)) {
                SortVal::$variant(v) => v,
                _ => unreachable!(concat!(
                    "sort_drive: ",
                    stringify!($work),
                    " must produce ",
                    stringify!($variant)
                )),
            }
        }
    };
}

sort_entry!(sort_par, Par, Par, Par);
sort_entry!(sort_expr, Expr, Expr, Expr);
sort_entry!(sort_send, Send, Send, Send);
sort_entry!(sort_receive, Receive, Receive, Receive);
sort_entry!(sort_bind_ref, Bind, Bind, ReceiveBind);
sort_entry!(sort_new, New, New, New);
sort_entry!(sort_match_node, Match, Match, Match);
sort_entry!(sort_if, If, If, If);
sort_entry!(sort_bundle, Bundle, Bundle, Bundle);
sort_entry!(sort_connective, Connective, Connective, Connective);
