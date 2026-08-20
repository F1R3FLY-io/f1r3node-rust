//! # Leg-2: the explicit-worklist driver for the substitution SCC
//!
//! Eight `SubstituteTrait` implementations (`Par`, `Send`, `Receive`, `New`,
//! `Match`, `If`, `Expr`, `Bundle`) plus the two folds `sub_exp` / `sub_conn`
//! form one mutually recursive post-order rewrite whose call depth is Θ(term
//! nesting depth). Measured at **195,728 bytes of native stack per bracket
//! level** in debug, that is depth 9 on the 2 MiB stack a tokio worker gets —
//! so `@"OUT"!([[[[[[[[[[0]]]]]]]]]])` aborts the node process. A stack
//! overflow is a `SIGSEGV`, not a catchable error: program-controlled nesting
//! depth therefore controls node liveness.
//!
//! This module replaces the call stack with an explicit heap worklist. Native
//! stack becomes `O(1)` in both nesting depth and sibling width; the recursion
//! lives in `work`.
//!
//! Full analysis, enumeration, measured constants and proof standard:
//! `docs/design/audits/theta-depth-traversals-2026-07-26.md`. The pattern is
//! the one the evaluator SCC already uses (`reduce.rs`, `EvVal` / `EvWork` /
//! `EvKont` / `eval_drive`, commit `a929a2d6`); it is copied rather than
//! reinvented so a reviewer sees the house standard.
//!
//! ## Structure
//!
//! * [`SubVal`] — a produced value, tagged by the producing entry point's
//!   return type.
//! * [`SubWork`] — a pending unit of work: substitute one owned node, take one
//!   step of a resumable fold, or run a post-order [`SubKont`].
//! * [`SubKont`] — the post-order continuation: it names the node being
//!   reassembled and carries the metadata that is *not* a child (flags, cached
//!   bitsets, counts, remainders).
//! * [`Substitute::sub_drive`] — the single LIFO loop.
//!
//! Work items own their subterms. Substitution *consumes* its input, so owning
//! is the natural fit and moving a `Par` onto the worklist is a 248-byte move,
//! never a deep copy.
//!
//! ## ★ The environment — why there is no owned `Env` per work item
//!
//! `Env::shift(j)` is
//!
//! ```ignore
//! Env { shift: self.shift + j, ..(*self).clone() }
//! ```
//!
//! — a full `HashMap<i32, Par>` clone, hence a `<Par as Clone>::clone` of every
//! bound value. `Par::clone` is itself Θ(depth) (15,875 B/level debug), so a
//! worklist that constructed an owned `Env` per item would remain Θ(depth) in
//! native stack **no matter how the traversal itself was written**. That is
//! measured, not assumed: under an environment binding a depth-`N` value the
//! binder-nesting probe costs 48,878 B/level against 33,242 B/level under a
//! ground binding — a difference of 15,636 B/level, i.e. `Par::clone`'s
//! constant to within 1.5% (`rholang/tests/stack_depth_probe.rs`, subjects
//! `subst_binders` and `subst_binders_ground_env`).
//!
//! The SCC's *entire* use of `Env` is three operations — `get`, reading the
//! `shift` field, and constructing `shift(j)`. There is **no `put`**. So a work
//! item carries only `(depth, shift_delta)` ([`SubCtx`]) alongside one borrowed
//! root environment, and
//!
//! ```text
//!   shift          ==  root.shift + shift_delta
//!   get(k)         ==  root.env_map[(root.level + root.shift + shift_delta) - k - 1]
//! ```
//!
//! which is exactly what `root.shift(Σ j).get(k)` computes, because `shift(j)`
//! changes only the `shift` field. Equality is by construction, not by test.
//!
//! ⚠ If `put` ever appears in this SCC the representation becomes wrong — `put`
//! changes `level` and `env_map`, which `shift_delta` cannot express — and
//! every `locally_free` bitset in the output would be wrong with it.
//! [`the_scc_never_puts_into_the_environment`] is the guard.
//!
//! ## Residual: the ONE Θ(depth) call that remains, and why
//!
//! `Env::get` returns its value **cloned**, because substituting a `BoundVar`
//! splices the bound term into the result and a binding may be used many times.
//! That clone is `<Par as Clone>::clone` — a derived impl over 39 `prost`
//! messages, row 5 of the audit's table, whose disposition is explicitly
//! "Leg-1 only: remove the call sites, not the impl". This call site cannot be
//! removed: the copy is the *meaning* of substitution. It is therefore an
//! irreducible residual of the same kind as `drop_in_place` (§7.2, §8.4 limit
//! #6), it is identical in the recursive form, and it is bounded by the depth
//! of the **bound value**, not of the term being traversed. The gate names it
//! separately (`substitute_deep_binding`) so that it is visible rather than
//! folded into a claim of full depth-independence.

use std::collections::BTreeMap;

use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{
    Bundle, Connective, ConnectiveBody, EVar, Expr, GUnforgeable, If, Match, MatchCase, New, Par,
    Receive, ReceiveBind, Send, Var, VarRef,
};
use models::rust::canonical_path::decode_trie_path;
use models::rust::epathmap_trie_codec::{EPathMapMode, EPathMapRepr};
use models::rust::par_bag::{Multiplicity, ParBagComponent};
use models::rust::par_construction::{ParConstruction, PlannedComponent, PlannedPar};
use models::rust::rhoapi_ext::{OwnedEPathMapEntries, OwnedEPathMapEntry};
use models::rust::utils::union;
use pathmap::PathMap;
use rspace_plus_plus::rspace::history::Either;

use super::env::Env;
use super::errors::InterpreterError;
use super::matcher::has_locally_free::{
    connective_connective_used_ref, connective_locally_free_ref, expr_connective_used_ref,
    expr_locally_free_ref,
};
use super::substitute::Substitute;
use super::substitute_combine::{
    expr_arm_pattern_slots, missing_required_field, rebuild_connective, rebuild_expr_shell,
    set_bits_until, split_expr_instance, BinaryArm, ConnArm, ExprArm,
};

// ===========================================================================
// the environment view
// ===========================================================================

/// The two numbers a work item has to carry: the pattern depth substitution is
/// running at, and the accumulated `Env::shift` delta.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SubCtx {
    pub depth: i32,
    pub shift_delta: i32,
}

impl SubCtx {
    pub fn root(depth: i32) -> Self {
        SubCtx {
            depth,
            shift_delta: 0,
        }
    }

    /// The recursive form's `&env.shift(j)`.
    pub fn shifted(self, j: i32) -> Self {
        SubCtx {
            depth: self.depth,
            shift_delta: self.shift_delta + j,
        }
    }

    /// The recursive form's `depth + 1` (pattern positions).
    pub fn deeper(self) -> Self {
        SubCtx {
            depth: self.depth + 1,
            shift_delta: self.shift_delta,
        }
    }
}

/// One borrowed root environment plus a per-item `shift_delta`, standing in for
/// the chain of owned `Env`s the recursive form allocates. See the module docs
/// for the equality argument.
#[derive(Clone, Copy)]
pub(crate) struct EnvView<'e> {
    root: &'e Env<Par>,
}

impl<'e> EnvView<'e> {
    pub fn new(root: &'e Env<Par>) -> Self { EnvView { root } }

    /// `env.shift(Σj).shift` — the field the `locally_free` truncation reads.
    pub fn shift(&self, ctx: SubCtx) -> i32 { self.root.shift + ctx.shift_delta }

    /// `env.shift(Σj).get(k)`.
    ///
    /// `Env::get` is `env_map[(level + shift) - k - 1].cloned()`, and `shift(j)`
    /// changes nothing but `shift`, so substituting `root.shift + Σj` for
    /// `shift` reproduces it exactly.
    pub fn get(&self, ctx: SubCtx, k: i32) -> Option<Par> {
        self.root
            .env_map
            .get(&((self.root.level + self.shift(ctx)) - k - 1))
            .cloned()
    }
}

// ===========================================================================
// values, work, continuations
// ===========================================================================

/// A produced value, tagged by the entry point that produced it.
pub(crate) enum SubVal {
    Par(Par),
    Expr(Expr),
    Send(Send),
    Receive(Receive),
    New(New),
    Match(Match),
    If(If),
    Bundle(Bundle),
}

struct PlannedComponentValue {
    component: PlannedComponent,
    locally_free: Vec<u8>,
    connective_used: bool,
}

struct PlannedBind {
    shell: ReceiveBind,
    children: Vec<PlannedPar>,
}

struct PlannedCase {
    shell: MatchCase,
    pattern: PlannedPar,
    source: PlannedPar,
    guard: PlannedPar,
}

enum PlannedSubVal {
    Par(PlannedPar),
    Expr(PlannedComponentValue),
    Send(PlannedComponent),
    Receive(PlannedComponent),
    New(PlannedComponent),
    Match(PlannedComponent),
    If(PlannedComponent),
    Bundle(PlannedComponent),
    Bind(PlannedBind),
    Case(PlannedCase),
}

/// A pending unit of work.
pub(crate) enum SubWork {
    /// A required `Option<Par>` slot: `None` raises the identical
    /// `unwrap_option_safe::<Par>` error the recursive form raises, at the
    /// identical point in the traversal.
    Par(Option<Par>, SubCtx),
    Expr(Expr, SubCtx),
    Send(Send, SubCtx),
    Receive(Receive, SubCtx),
    New(New, SubCtx),
    Match(Match, SubCtx),
    If(If, SubCtx),
    Bundle(Bundle, SubCtx),
    Bind(ReceiveBind, SubCtx),
    Case(MatchCase, SubCtx),
    /// Incremental EPathMap substitution. The owned cursor and rebuilt trie
    /// remain compressed between entries; only the current key/value becomes
    /// ordinary `Par` work.
    PathMap(Box<PathMapSubstitution>),
    /// One step of `sub_exp`'s resumable left fold. `rest` is stored REVERSED,
    /// so `pop()` yields the next element in source order.
    SubExp {
        rest: Vec<Expr>,
        acc: PlannedPar,
        ctx: SubCtx,
    },
    /// One step of `sub_conn`'s resumable left fold, `rest` likewise reversed.
    SubConn {
        rest: Vec<Connective>,
        acc: PlannedPar,
        ctx: SubCtx,
    },
    Combine(SubKont),
}

/// Post-order continuation: names the node being reassembled and carries the
/// metadata that is not a child value.
pub(crate) enum SubKont {
    /// Resume `sub_exp` after one `Expr` child completed.
    SubExpResume {
        rest: Vec<Expr>,
        acc: PlannedPar,
        ctx: SubCtx,
    },
    /// Resume `sub_conn` after `n` `Par` children of one connective completed.
    SubConnResume {
        rest: Vec<Connective>,
        acc: PlannedPar,
        arm: ConnArm,
        n: usize,
        ctx: SubCtx,
    },
    ExprArmK {
        arm: ExprArm,
        n: usize,
        depth: i32,
        shift: i32,
    },
    PathMapSetK(Box<PathMapSubstitution>),
    PathMapMapK(Box<PathMapSubstitution>),
    ParK {
        unforgeables: Vec<GUnforgeable>,
        locally_free: Vec<u8>,
        connective_used: bool,
        n_sends: usize,
        n_bundles: usize,
        n_receives: usize,
        n_news: usize,
        n_matches: usize,
        n_conditionals: usize,
        shift: i32,
    },
    SendK {
        persistent: bool,
        locally_free: Vec<u8>,
        connective_used: bool,
        n_data: usize,
        shift: i32,
    },
    BindK {
        remainder: Option<Var>,
        free_count: i32,
        n_patterns: usize,
    },
    ReceiveK {
        persistent: bool,
        peek: bool,
        bind_count: i32,
        locally_free: Vec<u8>,
        connective_used: bool,
        n_binds: usize,
        has_condition: bool,
        shift: i32,
    },
    NewK {
        bind_count: i32,
        uri: Vec<String>,
        injections: BTreeMap<String, Par>,
        locally_free: Vec<u8>,
        shift: i32,
    },
    CaseK {
        free_count: i32,
        has_guard: bool,
    },
    MatchK {
        locally_free: Vec<u8>,
        connective_used: bool,
        n_cases: usize,
        shift: i32,
    },
    IfK {
        locally_free: Vec<u8>,
        connective_used: bool,
        shift: i32,
    },
    BundleK {
        shell: Bundle,
    },
}

pub(crate) struct PathMapSubstitution {
    entries: OwnedEPathMapEntries,
    result: EPathMapRepr<PlannedPar>,
    locally_free: Vec<u8>,
    connective_used: bool,
    remainder: Option<Var>,
    ctx: SubCtx,
}

// ---------------------------------------------------------------------------
// value-stack pop helpers (type discipline: the producing `SubWork` guarantees
// the variant, so a mismatch is a driver bug and never an input error).
// ---------------------------------------------------------------------------

macro_rules! sub_pop {
    ($name:ident, $variant:ident, $ty:ty) => {
        #[inline]
        fn $name(vals: &mut Vec<PlannedSubVal>) -> $ty {
            match vals.pop() {
                Some(PlannedSubVal::$variant(v)) => v,
                _ => unreachable!(concat!(
                    "sub_drive: expected ",
                    stringify!($variant),
                    " on the value stack"
                )),
            }
        }
    };
}

sub_pop!(pop_par, Par, PlannedPar);
sub_pop!(pop_expr, Expr, PlannedComponentValue);
sub_pop!(pop_send, Send, PlannedComponent);
sub_pop!(pop_receive, Receive, PlannedComponent);
sub_pop!(pop_new, New, PlannedComponent);
sub_pop!(pop_match, Match, PlannedComponent);
sub_pop!(pop_if, If, PlannedComponent);
sub_pop!(pop_bundle, Bundle, PlannedComponent);
sub_pop!(pop_bind, Bind, PlannedBind);
sub_pop!(pop_case, Case, PlannedCase);

macro_rules! sub_pop_n {
    ($name:ident, $one:ident, $ty:ty) => {
        /// Pop `n` values, returning them in FORWARD (push) order.
        #[inline]
        fn $name(vals: &mut Vec<PlannedSubVal>, n: usize) -> Vec<$ty> {
            let mut out = Vec::with_capacity(n);
            for _ in 0..n {
                out.push($one(vals));
            }
            out.reverse();
            out
        }
    };
}

sub_pop_n!(pop_n_par, pop_par, PlannedPar);
sub_pop_n!(pop_n_send, pop_send, PlannedComponent);
sub_pop_n!(pop_n_receive, pop_receive, PlannedComponent);
sub_pop_n!(pop_n_new, pop_new, PlannedComponent);
sub_pop_n!(pop_n_match, pop_match, PlannedComponent);
sub_pop_n!(pop_n_bundle, pop_bundle, PlannedComponent);
sub_pop_n!(pop_n_if, pop_if, PlannedComponent);
sub_pop_n!(pop_n_bind, pop_bind, PlannedBind);
sub_pop_n!(pop_n_case, pop_case, PlannedCase);

fn finish_component<T>(
    construction: &mut ParConstruction,
    component: PlannedComponent,
) -> Result<T, InterpreterError>
where
    T: ParBagComponent + Clone,
{
    let root = construction.singleton_component(component, Vec::new(), false)?;
    let root = construction.finish(root)?;
    Ok(root
        .first::<T>()
        .expect("a planned component finishes to a canonical ParBag key")
        .expect("a planned singleton component remains present")
        .0)
}

fn finish_planned(
    construction: &mut ParConstruction,
    value: PlannedSubVal,
) -> Result<SubVal, InterpreterError> {
    Ok(match value {
        PlannedSubVal::Par(par) => SubVal::Par(construction.finish(par)?),
        PlannedSubVal::Expr(expr) => SubVal::Expr(finish_component(construction, expr.component)?),
        PlannedSubVal::Send(component) => SubVal::Send(finish_component(construction, component)?),
        PlannedSubVal::Receive(component) => {
            SubVal::Receive(finish_component(construction, component)?)
        }
        PlannedSubVal::New(component) => SubVal::New(finish_component(construction, component)?),
        PlannedSubVal::Match(component) => {
            SubVal::Match(finish_component(construction, component)?)
        }
        PlannedSubVal::If(component) => SubVal::If(finish_component(construction, component)?),
        PlannedSubVal::Bundle(component) => {
            SubVal::Bundle(finish_component(construction, component)?)
        }
        PlannedSubVal::Bind(_) | PlannedSubVal::Case(_) => {
            unreachable!("sub_drive: an interior planned value reached the public boundary")
        }
    })
}

/// Push `items` so that they are POPPED in forward order.
#[inline]
fn push_reversed<T, F: FnMut(T) -> SubWork>(work: &mut Vec<SubWork>, items: Vec<T>, mut f: F) {
    for item in items.into_iter().rev() {
        work.push(f(item));
    }
}

// ===========================================================================
// the shared, single-sourced variable resolvers
// ===========================================================================

/// `Substitute::maybe_substitute_var`, over an [`EnvView`].
///
/// Single-sourced: the public by-`Env` method delegates here with
/// `shift_delta = 0`, and both the driver and the recursive oracle call it, so
/// the "illegal substitution" error payload cannot drift between paths.
pub(crate) fn maybe_substitute_var_view(
    term: Var,
    ctx: SubCtx,
    env: EnvView<'_>,
) -> Result<Either<Var, Par>, InterpreterError> {
    if ctx.depth != 0 {
        return Ok(Either::Left(term));
    }
    match super::unwrap_option_safe(term.clone().var_instance)? {
        VarInstance::BoundVar(index) => match env.get(ctx, index) {
            Some(p) => Ok(Either::Right(p)),
            None => Ok(Either::Left(term)),
        },
        _ => Err(InterpreterError::SubstituteError(format!(
            "Illegal Substitution [{:?}]",
            term
        ))),
    }
}

/// `Substitute::maybe_substitute_evar`, over an [`EnvView`].
pub(crate) fn maybe_substitute_evar_view(
    term: EVar,
    ctx: SubCtx,
    env: EnvView<'_>,
) -> Result<Either<EVar, Par>, InterpreterError> {
    match maybe_substitute_var_view(super::unwrap_option_safe(term.v)?, ctx, env)? {
        Either::Left(v) => Ok(Either::Left(EVar { v: Some(v) })),
        Either::Right(p) => Ok(Either::Right(p)),
    }
}

/// `Substitute::maybe_substitute_var_ref`, over an [`EnvView`].
pub(crate) fn maybe_substitute_var_ref_view(
    term: VarRef,
    ctx: SubCtx,
    env: EnvView<'_>,
) -> Result<Either<VarRef, Par>, InterpreterError> {
    if term.depth != ctx.depth {
        return Ok(Either::Left(term));
    }
    match env.get(ctx, term.index) {
        Some(p) => Ok(Either::Right(p)),
        None => Ok(Either::Left(term)),
    }
}

// ===========================================================================
// the driver
// ===========================================================================

impl Substitute {
    /// The single driver loop. Native stack is `O(1)`; the recursion lives in
    /// `work`.
    ///
    /// A `?` abort discards `work` and `vals` — identical to the recursive
    /// form's `?`, which discards its pending frames. Nothing in this SCC
    /// charges (see the neutrality argument in the audit's §8.2), so there is
    /// no cost state to unwind.
    pub(crate) fn sub_drive(
        &self,
        root: SubWork,
        env: &Env<Par>,
    ) -> Result<SubVal, InterpreterError> {
        let view = EnvView::new(env);
        let mut construction = ParConstruction::new();
        let empty = construction.empty_par()?;
        let mut work: Vec<SubWork> = Vec::with_capacity(64);
        let mut vals: Vec<PlannedSubVal> = Vec::with_capacity(64);
        work.push(root);

        while let Some(w) = work.pop() {
            match w {
                SubWork::Par(slot, ctx) => {
                    let term = match slot {
                        Some(p) => p,
                        None => return Err(missing_required_field::<Par>()),
                    };
                    descend_par(term, ctx, view, empty, &mut work);
                }
                SubWork::Expr(term, ctx) => descend_expr(term, ctx, view, &mut work)?,
                SubWork::Send(term, ctx) => descend_send(term, ctx, view, &mut work),
                SubWork::Receive(term, ctx) => descend_receive(term, ctx, view, &mut work),
                SubWork::New(term, ctx) => descend_new(term, ctx, view, &mut work),
                SubWork::Match(term, ctx) => descend_match(term, ctx, view, &mut work),
                SubWork::If(term, ctx) => descend_if(term, ctx, view, &mut work),
                SubWork::Bundle(term, ctx) => descend_bundle(term, ctx, &mut work),
                SubWork::Bind(term, ctx) => descend_bind(term, ctx, &mut work),
                SubWork::Case(term, ctx) => descend_case(term, ctx, view, &mut work),
                SubWork::PathMap(state) => {
                    step_pathmap(state, &mut construction, &mut work, &mut vals)?
                }
                SubWork::SubExp { rest, acc, ctx } => step_sub_exp(
                    rest,
                    acc,
                    ctx,
                    view,
                    &mut construction,
                    &mut work,
                    &mut vals,
                )?,
                SubWork::SubConn { rest, acc, ctx } => step_sub_conn(
                    rest,
                    acc,
                    ctx,
                    view,
                    &mut construction,
                    &mut work,
                    &mut vals,
                )?,
                SubWork::Combine(k) => combine(k, empty, &mut construction, &mut work, &mut vals)?,
            }
        }

        let result = vals
            .pop()
            .expect("sub_drive: exactly one value must remain on the stack");
        debug_assert!(vals.is_empty(), "sub_drive leaves no sibling values");
        finish_planned(&mut construction, result)
    }
}

// ---------------------------------------------------------------------------
// descend_* — one per non-`Combine` work variant. Children are pushed in
// REVERSE so they pop in the order the recursive form substitutes them, which
// is what makes an early `?` return the identical error.
// ---------------------------------------------------------------------------

fn descend_par(
    mut term: Par,
    ctx: SubCtx,
    view: EnvView<'_>,
    empty: PlannedPar,
    work: &mut Vec<SubWork>,
) {
    let sends = term.take_occurrences::<Send>();
    let receives = term.take_occurrences::<Receive>();
    let news = term.take_occurrences::<New>();
    let exprs = term.take_occurrences::<Expr>();
    let matches = term.take_occurrences::<Match>();
    let unforgeables = term.take_occurrences::<GUnforgeable>();
    let bundles = term.take_occurrences::<Bundle>();
    let connectives = term.take_occurrences::<Connective>();
    let conditionals = term.take_occurrences::<If>();
    let locally_free = term.take_locally_free();
    let connective_used = term.connective_used();

    work.push(SubWork::Combine(SubKont::ParK {
        unforgeables,
        locally_free,
        connective_used,
        n_sends: sends.len(),
        n_bundles: bundles.len(),
        n_receives: receives.len(),
        n_news: news.len(),
        n_matches: matches.len(),
        n_conditionals: conditionals.len(),
        shift: view.shift(ctx),
    }));

    // Processing order is exprs, connectives, sends, bundles, receives, news,
    // matches, conditionals — `SubstituteTrait<Par>::substitute_no_sort`'s
    // statement order, which decides which `?` fires first.
    push_reversed(work, conditionals, |i| SubWork::If(i, ctx));
    push_reversed(work, matches, |m| SubWork::Match(m, ctx));
    push_reversed(work, news, |n| SubWork::New(n, ctx));
    push_reversed(work, receives, |r| SubWork::Receive(r, ctx));
    push_reversed(work, bundles, |b| SubWork::Bundle(b, ctx));
    push_reversed(work, sends, |s| SubWork::Send(s, ctx));

    let mut conn_rest = connectives;
    conn_rest.reverse();
    work.push(SubWork::SubConn {
        rest: conn_rest,
        acc: empty,
        ctx,
    });

    let mut expr_rest = exprs;
    expr_rest.reverse();
    work.push(SubWork::SubExp {
        rest: expr_rest,
        acc: empty,
        ctx,
    });
}

fn descend_expr(
    term: Expr,
    ctx: SubCtx,
    view: EnvView<'_>,
    work: &mut Vec<SubWork>,
) -> Result<(), InterpreterError> {
    let instance = super::unwrap_option_safe(term.expr_instance)?;
    if let ExprInstance::EPathmapBody(pathmap) = instance {
        let parts = pathmap.into_owned_parts();
        let mode = match &parts.entries {
            OwnedEPathMapEntries::Empty => EPathMapMode::Empty,
            OwnedEPathMapEntries::Set(_) => EPathMapMode::Set,
            OwnedEPathMapEntries::Map(_) => EPathMapMode::Map,
        };
        let result = match mode {
            EPathMapMode::Empty => EPathMapRepr::Empty,
            EPathMapMode::Set => EPathMapRepr::Set(PathMap::new()),
            EPathMapMode::Map => EPathMapRepr::Map(PathMap::new()),
        };
        work.push(SubWork::PathMap(Box::new(PathMapSubstitution {
            entries: parts.entries,
            result,
            locally_free: set_bits_until(parts.locally_free, view.shift(ctx)),
            connective_used: parts.connective_used,
            remainder: parts.remainder,
            ctx,
        })));
        return Ok(());
    }
    let (arm, children) = split_expr_instance(instance);

    // `unwrap_option_safe` fires per operand, interleaved with the descent — p1
    // is checked, substituted, THEN p2 is checked. The child list keeps absent
    // operands as `None` in position and `SubWork::Par` raises the error when
    // it pops one, so the interleaving is preserved exactly.
    let n = children.len();
    // Pattern slots are visited at `depth + 1` (`EMatches::pattern` today).
    // Read before `arm` is moved into the continuation.
    let pattern_slots = expr_arm_pattern_slots(&arm);
    work.push(SubWork::Combine(SubKont::ExprArmK {
        arm,
        n,
        depth: ctx.depth,
        shift: view.shift(ctx),
    }));
    // Reversed, so the slots pop in `split_expr_instance` order; the index is
    // the slot's ORIGINAL position, which is what `pattern_slots` names.
    for (slot, child) in children.into_iter().enumerate().rev() {
        let slot_ctx = match pattern_slots.contains(&slot) {
            true => ctx.deeper(),
            false => ctx,
        };
        work.push(SubWork::Par(child, slot_ctx));
    }
    Ok(())
}

fn step_pathmap(
    mut state: Box<PathMapSubstitution>,
    construction: &mut ParConstruction,
    work: &mut Vec<SubWork>,
    vals: &mut Vec<PlannedSubVal>,
) -> Result<(), InterpreterError> {
    match state.entries.next() {
        Some(OwnedEPathMapEntry::Set(key)) => {
            let entry =
                decode_trie_path(&key).expect("set-mode EPathMap keys are canonical Par paths");
            let ctx = state.ctx;
            work.push(SubWork::Combine(SubKont::PathMapSetK(state)));
            work.push(SubWork::Par(Some(entry), ctx));
        }
        Some(OwnedEPathMapEntry::Map { key, value }) => {
            let entry_key =
                decode_trie_path(&key).expect("map-mode EPathMap keys are canonical Par paths");
            let ctx = state.ctx;
            work.push(SubWork::Combine(SubKont::PathMapMapK(state)));
            // LIFO: key is substituted before its associated value.
            work.push(SubWork::Par(Some(value), ctx));
            work.push(SubWork::Par(Some(entry_key), ctx));
        }
        None => {
            let component = construction.epathmap_repr_expr_component(
                state.result,
                state.locally_free.clone(),
                state.connective_used,
                state.remainder,
            )?;
            vals.push(PlannedSubVal::Expr(PlannedComponentValue {
                component,
                locally_free: state.locally_free,
                connective_used: state.connective_used,
            }));
        }
    }
    Ok(())
}

fn descend_send(term: Send, ctx: SubCtx, view: EnvView<'_>, work: &mut Vec<SubWork>) {
    let Send {
        chan,
        data,
        persistent,
        locally_free,
        connective_used,
    } = term;
    work.push(SubWork::Combine(SubKont::SendK {
        persistent,
        locally_free,
        connective_used,
        n_data: data.len(),
        shift: view.shift(ctx),
    }));
    push_reversed(work, data, |p| SubWork::Par(Some(p), ctx));
    work.push(SubWork::Par(chan, ctx));
}

fn descend_bind(term: ReceiveBind, ctx: SubCtx, work: &mut Vec<SubWork>) {
    let ReceiveBind {
        patterns,
        source,
        remainder,
        free_count,
    } = term;
    work.push(SubWork::Combine(SubKont::BindK {
        remainder,
        free_count,
        n_patterns: patterns.len(),
    }));
    // Patterns are substituted at `depth + 1`; the source channel is not.
    let deeper = ctx.deeper();
    push_reversed(work, patterns, move |p| SubWork::Par(Some(p), deeper));
    work.push(SubWork::Par(source, ctx));
}

fn descend_receive(term: Receive, ctx: SubCtx, view: EnvView<'_>, work: &mut Vec<SubWork>) {
    let Receive {
        binds,
        body,
        persistent,
        peek,
        bind_count,
        locally_free,
        connective_used,
        condition,
    } = term;
    let has_condition = condition.is_some();
    work.push(SubWork::Combine(SubKont::ReceiveK {
        persistent,
        peek,
        bind_count,
        locally_free,
        connective_used,
        n_binds: binds.len(),
        has_condition,
        shift: view.shift(ctx),
    }));
    // Order: every bind, then the body, then the condition. Body and condition
    // are substituted under `env.shift(bind_count)`.
    let shifted = ctx.shifted(bind_count);
    if let Some(c) = condition {
        work.push(SubWork::Par(Some(c), shifted));
    }
    work.push(SubWork::Par(body, shifted));
    push_reversed(work, binds, |b| SubWork::Bind(b, ctx));
}

fn descend_new(term: New, ctx: SubCtx, view: EnvView<'_>, work: &mut Vec<SubWork>) {
    let New {
        bind_count,
        p,
        uri,
        injections,
        locally_free,
    } = term;
    work.push(SubWork::Combine(SubKont::NewK {
        bind_count,
        uri,
        injections,
        locally_free,
        shift: view.shift(ctx),
    }));
    work.push(SubWork::Par(p, ctx.shifted(bind_count)));
}

fn descend_case(term: MatchCase, ctx: SubCtx, _view: EnvView<'_>, work: &mut Vec<SubWork>) {
    let MatchCase {
        pattern,
        source,
        free_count,
        guard,
    } = term;
    let has_guard = guard.is_some();
    work.push(SubWork::Combine(SubKont::CaseK {
        free_count,
        has_guard,
    }));
    // Order: source (shifted by `free_count`), then pattern (at `depth + 1`,
    // UNSHIFTED), then the guard (shifted).
    let shifted = ctx.shifted(free_count);
    if let Some(g) = guard {
        work.push(SubWork::Par(Some(g), shifted));
    }
    work.push(SubWork::Par(pattern, ctx.deeper()));
    work.push(SubWork::Par(source, shifted));
}

fn descend_match(term: Match, ctx: SubCtx, view: EnvView<'_>, work: &mut Vec<SubWork>) {
    let Match {
        target,
        cases,
        locally_free,
        connective_used,
    } = term;
    // ⚠ The recursive form FILTERS cases with a missing pattern or source out
    // of the result rather than erroring on them. Reproduced verbatim.
    let cases: Vec<MatchCase> = cases
        .into_iter()
        .filter(|case| case.pattern.is_some() && case.source.is_some())
        .collect();
    work.push(SubWork::Combine(SubKont::MatchK {
        locally_free,
        connective_used,
        n_cases: cases.len(),
        shift: view.shift(ctx),
    }));
    push_reversed(work, cases, |c| SubWork::Case(c, ctx));
    work.push(SubWork::Par(target, ctx));
}

fn descend_if(term: If, ctx: SubCtx, view: EnvView<'_>, work: &mut Vec<SubWork>) {
    let If {
        condition,
        if_true,
        if_false,
        locally_free,
        connective_used,
    } = term;
    work.push(SubWork::Combine(SubKont::IfK {
        locally_free,
        connective_used,
        shift: view.shift(ctx),
    }));
    work.push(SubWork::Par(if_false, ctx));
    work.push(SubWork::Par(if_true, ctx));
    work.push(SubWork::Par(condition, ctx));
}

fn descend_bundle(mut term: Bundle, ctx: SubCtx, work: &mut Vec<SubWork>) {
    // Leg-1: `body` is moved out with `Option::take`; `single_bundle` /
    // `BundleOps::merge` only ever READ the shell's flags, which `take` leaves
    // intact.
    let body = term.body.take();
    work.push(SubWork::Combine(SubKont::BundleK { shell: term }));
    work.push(SubWork::Par(body, ctx));
}

// ---------------------------------------------------------------------------
// the two resumable folds
// ---------------------------------------------------------------------------

fn step_sub_exp(
    mut rest: Vec<Expr>,
    acc: PlannedPar,
    ctx: SubCtx,
    view: EnvView<'_>,
    construction: &mut ParConstruction,
    work: &mut Vec<SubWork>,
    vals: &mut Vec<PlannedSubVal>,
) -> Result<(), InterpreterError> {
    let Some(expr) = rest.pop() else {
        vals.push(PlannedSubVal::Par(acc));
        return Ok(());
    };

    match expr.expr_instance {
        None => {
            // Preserves `unwrap_option_safe`'s
            // `UndefinedRequiredProtobufFieldError("\"…ExprInstance\"")`
            // payload byte-for-byte, raised at the same point.
            Err(missing_required_field::<ExprInstance>())
        }
        Some(ExprInstance::EVarBody(e)) => {
            let acc = match maybe_substitute_evar_view(e, ctx, view)? {
                Either::Left(e) => {
                    let expr = Expr {
                        expr_instance: Some(ExprInstance::EVarBody(e)),
                    };
                    let value = PlannedComponentValue {
                        component: construction.expr_component(&expr, &[], false)?,
                        locally_free: expr_locally_free_ref(&expr, ctx.depth),
                        connective_used: expr_connective_used_ref(&expr),
                    };
                    append_planned_component(construction, acc, value)?
                }
                Either::Right(p) => {
                    let substituted = construction.import_par(&p)?;
                    construction.parallel(substituted, acc)?
                }
            };
            work.push(SubWork::SubExp { rest, acc, ctx });
            Ok(())
        }
        Some(other) => {
            work.push(SubWork::Combine(SubKont::SubExpResume { rest, acc, ctx }));
            work.push(SubWork::Expr(
                Expr {
                    expr_instance: Some(other),
                },
                ctx,
            ));
            Ok(())
        }
    }
}

fn step_sub_conn(
    mut rest: Vec<Connective>,
    acc: PlannedPar,
    ctx: SubCtx,
    view: EnvView<'_>,
    construction: &mut ParConstruction,
    work: &mut Vec<SubWork>,
    vals: &mut Vec<PlannedSubVal>,
) -> Result<(), InterpreterError> {
    let Some(conn) = rest.pop() else {
        vals.push(PlannedSubVal::Par(acc));
        return Ok(());
    };

    // `None => Ok(par)` — the element contributes nothing and is skipped.
    let Some(instance) = conn.connective_instance else {
        work.push(SubWork::SubConn { rest, acc, ctx });
        return Ok(());
    };

    match instance {
        ConnectiveInstance::VarRefBody(v) => {
            let acc = match maybe_substitute_var_ref_view(v, ctx, view)? {
                // `VarRef` is `Copy`, so rebuilding the connective from it is
                // byte-identical to the original the recursive form prepends.
                Either::Left(_) => {
                    let connective = Connective {
                        connective_instance: Some(ConnectiveInstance::VarRefBody(v)),
                    };
                    let value = plan_connective(construction, &connective, &[], ctx.depth)?;
                    append_planned_component(construction, acc, value)?
                }
                Either::Right(new_par) => {
                    let substituted = construction.import_par(&new_par)?;
                    construction.parallel(substituted, acc)?
                }
            };
            work.push(SubWork::SubConn { rest, acc, ctx });
        }
        ConnectiveInstance::ConnAndBody(ConnectiveBody { ps }) => {
            push_conn_children(work, rest, acc, ConnArm::ConnAnd, ps, ctx);
        }
        ConnectiveInstance::ConnOrBody(ConnectiveBody { ps }) => {
            push_conn_children(work, rest, acc, ConnArm::ConnOr, ps, ctx);
        }
        ConnectiveInstance::ConnNotBody(p) => {
            push_conn_children(work, rest, acc, ConnArm::ConnNot, vec![p], ctx);
        }
        ground @ (ConnectiveInstance::ConnBool(_)
        | ConnectiveInstance::ConnInt(_)
        | ConnectiveInstance::ConnString(_)
        | ConnectiveInstance::ConnUri(_)
        | ConnectiveInstance::ConnByteArray(_)) => {
            // ⚠ Through `rebuild_connective`, not `Connective { .. }` inline.
            // The two are the same bytes — `ConnArm::Ground(i)` rebuilds to
            // exactly `Connective { connective_instance: Some(i) }` and takes
            // no children — but they were two SPELLINGS of "rebuild a
            // connective", one per lane: the oracle went through
            // `rebuild_connective` for every arm including this one, and this
            // driver went around it for this arm only.
            //
            // That asymmetry was load-bearing in the wrong direction.
            // `substitute_oracle` is `#[cfg(test)]`, so `ConnArm::Ground` had
            // no construction site in a lib build at all and
            // `cargo clippy --workspace` reported the variant as never
            // constructed — under `-D warnings`, a red gate pointing at a
            // variant that is in fact reachable, just not from the lane that
            // ships. Routing this arm through the shared function makes
            // `rebuild_connective` what its name says: the single place a
            // `Connective` is rebuilt from its arm, in both lanes.
            let connective = rebuild_connective(ConnArm::Ground(ground), Vec::new());
            let value = plan_connective(construction, &connective, &[], ctx.depth)?;
            let acc = append_planned_component(construction, acc, value)?;
            work.push(SubWork::SubConn { rest, acc, ctx });
        }
    }
    Ok(())
}

fn push_conn_children(
    work: &mut Vec<SubWork>,
    rest: Vec<Connective>,
    acc: PlannedPar,
    arm: ConnArm,
    ps: Vec<Par>,
    ctx: SubCtx,
) {
    let n = ps.len();
    work.push(SubWork::Combine(SubKont::SubConnResume {
        rest,
        acc,
        arm,
        n,
        ctx,
    }));
    push_reversed(work, ps, |p| SubWork::Par(Some(p), ctx));
}

fn append_planned_component(
    construction: &mut ParConstruction,
    acc: PlannedPar,
    value: PlannedComponentValue,
) -> Result<PlannedPar, InterpreterError> {
    Ok(construction.append_component(
        acc,
        value.component,
        value.locally_free,
        value.connective_used,
    )?)
}

fn plan_connective(
    construction: &mut ParConstruction,
    shell: &Connective,
    children: &[PlannedPar],
    depth: i32,
) -> Result<PlannedComponentValue, InterpreterError> {
    Ok(PlannedComponentValue {
        component: construction.connective_component(shell, children)?,
        locally_free: connective_locally_free_ref(shell, depth),
        connective_used: connective_connective_used_ref(shell),
    })
}

fn plan_expr(
    construction: &mut ParConstruction,
    arm: ExprArm,
    mut children: Vec<PlannedPar>,
    depth: i32,
    shift: i32,
) -> Result<PlannedComponentValue, InterpreterError> {
    let metadata = match &arm {
        ExprArm::Unary(_) => Some((
            construction.locally_free(children[0]).to_vec(),
            construction.connective_used(children[0]),
        )),
        ExprArm::Binary(kind) => Some((
            union(
                construction.locally_free(children[0]).to_vec(),
                construction.locally_free(children[1]).to_vec(),
            ),
            match kind {
                BinaryArm::EMatches => construction.connective_used(children[0]),
                _ => {
                    construction.connective_used(children[0])
                        || construction.connective_used(children[1])
                }
            },
        )),
        _ => None,
    };
    let opaque_no_descent = matches!(
        &arm,
        ExprArm::NoDescent(ExprInstance::EPathmapBody(_) | ExprInstance::EZipperBody(_))
    );
    let method = matches!(&arm, ExprArm::EMethod { .. });
    let shell = Expr {
        expr_instance: Some(rebuild_expr_shell(arm, children.len(), shift)),
    };
    let (locally_free, connective_used) = metadata.unwrap_or_else(|| {
        (
            expr_locally_free_ref(&shell, depth),
            expr_connective_used_ref(&shell),
        )
    });
    let component = if opaque_no_descent {
        construction.import_component(&shell)?
    } else {
        if method {
            children.rotate_left(1);
        }
        construction.expr_component(&shell, &children, false)?
    };
    Ok(PlannedComponentValue {
        component,
        locally_free,
        connective_used,
    })
}

// ---------------------------------------------------------------------------
// combine — the post-order bodies, every one delegating to `substitute_combine`
// ---------------------------------------------------------------------------

fn combine(
    k: SubKont,
    empty: PlannedPar,
    construction: &mut ParConstruction,
    work: &mut Vec<SubWork>,
    vals: &mut Vec<PlannedSubVal>,
) -> Result<(), InterpreterError> {
    match k {
        SubKont::SubExpResume { rest, acc, ctx } => {
            let e = pop_expr(vals);
            let acc = append_planned_component(construction, acc, e)?;
            work.push(SubWork::SubExp { rest, acc, ctx });
        }
        SubKont::SubConnResume {
            rest,
            acc,
            arm,
            n,
            ctx,
        } => {
            let children = pop_n_par(vals, n);
            let shell = rebuild_connective(arm, vec![Par::default(); children.len()]);
            let connective = plan_connective(construction, &shell, &children, ctx.depth)?;
            let acc = append_planned_component(construction, acc, connective)?;
            work.push(SubWork::SubConn { rest, acc, ctx });
        }
        SubKont::ExprArmK {
            arm,
            n,
            depth,
            shift,
        } => {
            let children = pop_n_par(vals, n);
            vals.push(PlannedSubVal::Expr(plan_expr(
                construction,
                arm,
                children,
                depth,
                shift,
            )?));
        }
        SubKont::PathMapSetK(mut state) => {
            let key = construction.canonical_path_bytes(pop_par(vals))?;
            match &mut state.result {
                EPathMapRepr::Set(entries) => {
                    entries.insert(&key, ());
                }
                EPathMapRepr::Empty | EPathMapRepr::Map(_) => {
                    unreachable!("set-mode EPathMap substitution changed representation")
                }
            }
            work.push(SubWork::PathMap(state));
        }
        SubKont::PathMapMapK(mut state) => {
            let value = pop_par(vals);
            let key = pop_par(vals);
            let key = construction.canonical_path_bytes(key)?;
            match &mut state.result {
                EPathMapRepr::Map(entries) => {
                    entries.insert(&key, value);
                }
                EPathMapRepr::Empty | EPathMapRepr::Set(_) => {
                    unreachable!("map-mode EPathMap substitution changed representation")
                }
            }
            work.push(SubWork::PathMap(state));
        }
        SubKont::ParK {
            unforgeables,
            locally_free,
            connective_used,
            n_sends,
            n_bundles,
            n_receives,
            n_news,
            n_matches,
            n_conditionals,
            shift,
        } => {
            // Popped in reverse push order.
            let conditionals = pop_n_if(vals, n_conditionals);
            let matches = pop_n_match(vals, n_matches);
            let news = pop_n_new(vals, n_news);
            let receives = pop_n_receive(vals, n_receives);
            let bundles = pop_n_bundle(vals, n_bundles);
            let sends = pop_n_send(vals, n_sends);
            let connectives_par = pop_par(vals);
            let exprs_par = pop_par(vals);
            let unforgeables = unforgeables
                .into_iter()
                .map(|unforgeable| construction.unforgeable_component(&unforgeable))
                .collect::<Result<Vec<_>, _>>()?;
            let occurrences = sends
                .into_iter()
                .chain(receives)
                .chain(news)
                .chain(matches)
                .chain(bundles)
                .chain(conditionals)
                .chain(unforgeables)
                .map(|component| (component, Multiplicity::ONE));
            let shell = construction.par(
                occurrences,
                set_bits_until(locally_free, shift),
                connective_used,
            )?;
            let root = construction.parallel(connectives_par, shell)?;
            let root = construction.parallel(exprs_par, root)?;
            vals.push(PlannedSubVal::Par(root));
        }
        SubKont::SendK {
            persistent,
            locally_free,
            connective_used,
            n_data,
            shift,
        } => {
            let data = pop_n_par(vals, n_data);
            let chan = pop_par(vals);
            let shell = Send {
                chan: Some(Par::default()),
                data: vec![Par::default(); data.len()],
                persistent,
                locally_free: set_bits_until(locally_free, shift),
                connective_used,
            };
            let children: Vec<_> = std::iter::once(chan).chain(data).collect();
            vals.push(PlannedSubVal::Send(
                construction.send_component(&shell, &children)?,
            ));
        }
        SubKont::BindK {
            remainder,
            free_count,
            n_patterns,
        } => {
            let patterns = pop_n_par(vals, n_patterns);
            let source = pop_par(vals);
            let shell = ReceiveBind {
                patterns: vec![Par::default(); patterns.len()],
                source: Some(Par::default()),
                remainder,
                free_count,
            };
            let children = patterns
                .into_iter()
                .chain(std::iter::once(source))
                .collect();
            vals.push(PlannedSubVal::Bind(PlannedBind { shell, children }));
        }
        SubKont::ReceiveK {
            persistent,
            peek,
            bind_count,
            locally_free,
            connective_used,
            n_binds,
            has_condition,
            shift,
        } => {
            let condition = if has_condition { pop_par(vals) } else { empty };
            let body = pop_par(vals);
            let binds = pop_n_bind(vals, n_binds);
            let mut bind_shells = Vec::with_capacity(binds.len());
            let mut children = Vec::new();
            for bind in binds {
                bind_shells.push(bind.shell);
                children.extend(bind.children);
            }
            children.extend([body, condition]);
            let shell = Receive {
                binds: bind_shells,
                body: Some(Par::default()),
                persistent,
                peek,
                bind_count,
                locally_free: set_bits_until(locally_free, shift),
                connective_used,
                condition: has_condition.then(Par::default),
            };
            vals.push(PlannedSubVal::Receive(
                construction.receive_component(&shell, &children)?,
            ));
        }
        SubKont::NewK {
            bind_count,
            uri,
            injections,
            locally_free,
            shift,
        } => {
            let body = pop_par(vals);
            let mut children = Vec::with_capacity(injections.len() + 1);
            children.push(body);
            let mut shell_injections = BTreeMap::new();
            for (key, value) in injections {
                children.push(construction.import_par(&value)?);
                shell_injections.insert(key, Par::default());
            }
            let shell = New {
                bind_count,
                p: Some(Par::default()),
                uri,
                injections: shell_injections,
                locally_free: set_bits_until(locally_free, shift),
            };
            vals.push(PlannedSubVal::New(
                construction.new_component(&shell, &children)?,
            ));
        }
        SubKont::CaseK {
            free_count,
            has_guard,
        } => {
            let guard = if has_guard { pop_par(vals) } else { empty };
            let pattern = pop_par(vals);
            let source = pop_par(vals);
            vals.push(PlannedSubVal::Case(PlannedCase {
                shell: MatchCase {
                    pattern: Some(Par::default()),
                    source: Some(Par::default()),
                    free_count,
                    guard: has_guard.then(Par::default),
                },
                pattern,
                source,
                guard,
            }));
        }
        SubKont::MatchK {
            locally_free,
            connective_used,
            n_cases,
            shift,
        } => {
            let cases = pop_n_case(vals, n_cases);
            let target = pop_par(vals);
            let mut case_shells = Vec::with_capacity(cases.len());
            let mut children = Vec::with_capacity(1 + cases.len() * 3);
            children.push(target);
            for case in cases {
                case_shells.push(case.shell);
                children.extend([case.pattern, case.source, case.guard]);
            }
            let shell = Match {
                target: Some(Par::default()),
                cases: case_shells,
                locally_free: set_bits_until(locally_free, shift),
                connective_used,
            };
            vals.push(PlannedSubVal::Match(
                construction.match_component(&shell, &children)?,
            ));
        }
        SubKont::IfK {
            locally_free,
            connective_used,
            shift,
        } => {
            let if_false = pop_par(vals);
            let if_true = pop_par(vals);
            let condition = pop_par(vals);
            let shell = If {
                condition: Some(Par::default()),
                if_true: Some(Par::default()),
                if_false: Some(Par::default()),
                locally_free: set_bits_until(locally_free, shift),
                connective_used,
            };
            vals.push(PlannedSubVal::If(
                construction.if_component(&shell, &[condition, if_true, if_false])?,
            ));
        }
        SubKont::BundleK { mut shell } => {
            let body = pop_par(vals);
            let body = match construction.single_bundle(body) {
                Some((inner, write_flag, read_flag)) => {
                    shell.write_flag &= write_flag;
                    shell.read_flag &= read_flag;
                    inner
                }
                None => body,
            };
            shell.body = Some(Par::default());
            vals.push(PlannedSubVal::Bundle(
                construction.bundle_component(&shell, &[body])?,
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// the `SubCtx` representation guard
// ---------------------------------------------------------------------------

#[cfg(test)]
mod representation_guard {
    //! ⚠ The `(depth, shift_delta)` + borrowed-root representation is valid
    //! **only** while the substitution SCC's entire use of `Env` is `get`, the
    //! `shift` field, and `shift(j)`. `Env::put` changes `level` and inserts
    //! into `env_map`; neither is expressible as a `shift_delta`, and every
    //! `locally_free` bitset in the output would silently be computed against
    //! the wrong shift.
    //!
    //! This is checked mechanically rather than by review, because it is the
    //! single assumption the whole conversion rests on.

    /// The SCC's source files, read at test time.
    const SCC_SOURCES: [(&str, &str); 3] = [
        ("substitute.rs", include_str!("substitute.rs")),
        ("substitute_drive.rs", include_str!("substitute_drive.rs")),
        (
            "substitute_combine.rs",
            include_str!("substitute_combine.rs"),
        ),
    ];

    #[test]
    fn the_scc_never_puts_into_the_environment() {
        for (name, source) in SCC_SOURCES {
            // Scan PRODUCTION code only: everything from the first
            // `#[cfg(test)]` onwards is test scaffolding, and the guard's own
            // assertion text (plus the test that deliberately builds an
            // environment with `put`) lives there.
            let production = match source.find("#[cfg(test)]") {
                Some(at) => &source[..at],
                None => source,
            };
            for (lineno, line) in production.lines().enumerate() {
                let code = line.trim_start();
                // Skip documentation, which names `put` when explaining why it
                // must not appear.
                if code.starts_with("//") || code.starts_with("*") {
                    continue;
                }
                assert!(
                    !code.contains(".put("),
                    "{}:{} calls `.put(` inside the substitution SCC:\n    {}\n\
                     The worklist represents the environment as (borrowed root, shift_delta), \
                     which can express `Env::shift` but NOT `Env::put` — `put` changes `level` \
                     and `env_map`. If this call is genuinely needed the representation is \
                     wrong and every `locally_free` bitset in the output is wrong with it. \
                     See the module documentation in substitute_drive.rs.",
                    name,
                    lineno + 1,
                    code
                );
            }
        }
    }

    /// The equality the representation rests on, stated as a test rather than
    /// only as prose: for any root environment and any chain of shifts, the
    /// view's `shift` and `get` agree with the owned `Env` the recursive form
    /// would have built.
    #[test]
    fn the_view_agrees_with_a_chain_of_owned_shifts() {
        use models::rhoapi::Par;

        use super::{EnvView, SubCtx};
        use crate::rust::interpreter::env::Env;
        use crate::rust::interpreter::test_utils::substitution_corpus::{gint, nested_list};

        let mut root: Env<Par> = Env::new();
        let mut root = root.put(gint(1));
        let mut root = root.put(nested_list(2));
        let root = root.put(gint(3));

        let view = EnvView::new(&root);

        for chain in [vec![], vec![1], vec![2, 3], vec![0, 1, 0, 5], vec![
            7, 11, 13,
        ]] {
            // The recursive form's owned environment after this chain.
            let mut owned = root.clone();
            let mut ctx = SubCtx::root(0);
            for j in &chain {
                owned = owned.shift(*j);
                ctx = ctx.shifted(*j);
            }

            assert_eq!(
                view.shift(ctx),
                owned.shift,
                "EnvView::shift diverged from Env::shift after the chain {:?}",
                chain
            );
            for k in -2i32..6 {
                assert_eq!(
                    view.get(ctx, k),
                    owned.get(&k),
                    "EnvView::get({}) diverged from Env::get after the chain {:?}",
                    k,
                    chain
                );
            }
        }
    }
}
