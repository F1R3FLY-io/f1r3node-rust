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
//! The authoritative `ParBag` boundary is consumed one distinct trie endpoint
//! at a time. Each endpoint's canonical protobuf body is copied once into a
//! shared [`ProtobufRegion`]; nested `Par`s are then constant-size byte-range
//! handles into that backing allocation. The worklist therefore carries neither
//! a decoded recursive tree nor one item per Bag occurrence. Multiplicity is
//! propagated algebraically when a component is rebuilt. EPathMap values are
//! the same kind of retained regions, paired with the shallow decoder's ACT
//! topology and consumed through the native owned PathMap cursor.
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
use models::rhoapi::cost_signature_atom::Form;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{
    Bundle, Connective, ConnectiveBody, CostRegion, CostSignature, CostTokenStack, EVar, Expr,
    GUnforgeable, If, Match, MatchCase, New, Par, Receive, ReceiveBind, Send, Var, VarRef,
};
use models::rust::canonical_path::{
    tag as path_tag, CanonicalTopLevelPathForm, CanonicalTopLevelSegment,
    CanonicalTopLevelSegmentCursor,
};
use models::rust::epathmap_trie_codec::{EPathMapMode, EPathMapRepr};
use models::rust::par_bag::{
    canonical_expr_oneof_tag, Multiplicity, OwnedCanonicalEzipper, ParBagComponent,
};
use models::rust::par_construction::{ParConstruction, PlannedComponent, PlannedPar};
use models::rust::rhoapi_ext::{OwnedEPathMapEntries, OwnedEPathMapEntry};
use models::rust::rholang::match_schema_tables::{
    BUNDLE_RHO_MATCH_PROGRAM, CONNECTIVE_RHO_MATCH_PROGRAM, COSTTOKENSTACK_RHO_MATCH_PROGRAM,
    EXPR_RHO_MATCH_PROGRAM, GUNFORGEABLE_RHO_MATCH_PROGRAM, IF_RHO_MATCH_PROGRAM,
    MATCH_RHO_MATCH_PROGRAM, NEW_RHO_MATCH_PROGRAM, RECEIVE_RHO_MATCH_PROGRAM,
    SEND_RHO_MATCH_PROGRAM,
};
use models::rust::rholang::protobuf_cursor::{
    CanonicalByteRange, CanonicalParRegionIndex, CanonicalRegionDelimitedRun, ProtobufRegion,
};
use models::rust::rholang::protobuf_decoder;
use models::rust::rholang::protobuf_region::{cut_at_par, ProtobufCutPlan};
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;
use models::rust::utils::union;
use pathmap::PathMap;
use prost::Message;
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

pub(crate) fn substitute_cost_signature(
    signer: &mut CostSignature,
    ctx: SubCtx,
    view: EnvView<'_>,
) {
    if ctx.depth != 0 {
        return;
    }
    for atom in &mut signer.atoms {
        let Some(Form::Bound(bound)) = atom.form.as_ref() else {
            continue;
        };
        if let Some(name) = view.get(ctx, bound.index) {
            atom.form = Some(Form::RuntimeName(
                ParSortMatcher::sort_match(&name).term.encode_to_vec(),
            ));
        }
    }
}

pub(crate) fn substitute_cost_region(
    region: &mut Option<CostRegion>,
    ctx: SubCtx,
    view: EnvView<'_>,
) {
    if let Some(signer) = region.as_mut().and_then(|region| region.signer.as_mut()) {
        substitute_cost_signature(signer, ctx, view);
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
    CanonicalKey(Vec<u8>),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EncodedFamily {
    Expr,
    Connective,
    Send,
    Bundle,
    Receive,
    New,
    Match,
    If,
    Unforgeable,
    CostTokenStack,
}

pub(crate) struct EncodedOccurrence {
    family: EncodedFamily,
    region: Option<ProtobufRegion>,
    canonical_ezipper: Option<OwnedCanonicalEzipper>,
    multiplicity: Multiplicity,
}

pub(crate) struct RegionParSubstitution {
    rest: Vec<EncodedOccurrence>,
    acc: PlannedPar,
    locally_free: Vec<u8>,
    connective_used: bool,
    ctx: SubCtx,
    preserve: bool,
}

enum RegionChild {
    Substitute(ProtobufRegion, SubCtx),
    Preserve(ProtobufRegion),
    Missing(SubCtx),
    Empty(PlannedPar),
}

enum RegionComponentShell {
    Expr {
        arm: ExprArm,
        preserve_shell: Option<Expr>,
        depth: i32,
        shift: i32,
    },
    Connective {
        shell: Connective,
        depth: i32,
    },
    Send(Send),
    Bundle(Bundle),
    Receive(Receive),
    New(New),
    Match(Match),
    If(If),
}

pub(crate) struct RegionComponentKont {
    shell: RegionComponentShell,
    multiplicity: Multiplicity,
    child_count: usize,
    /// Generated protobuf child order as indices into semantic evaluation
    /// order. Empty means the two orders are identical.
    wire_order: Vec<usize>,
    /// Identity-import mode bypasses every substitution-only rewrite.
    preserve: bool,
}

/// A pending unit of work.
pub(crate) enum SubWork {
    /// A required `Option<Par>` slot: `None` raises the identical
    /// `unwrap_option_safe::<Par>` error the recursive form raises, at the
    /// identical point in the traversal.
    Par(Option<Par>, SubCtx),
    /// A canonical protobuf `Par` body retained as one shared byte region.
    ParRegion(ProtobufRegion, SubCtx),
    /// Import an intentionally unsubstituted child through the same iterative
    /// region PDA. New injections use this semantic boundary.
    ImportParRegion(ProtobufRegion),
    PlannedPar(PlannedPar),
    CanonicalKey(Vec<u8>),
    RegionComponent(EncodedOccurrence, SubCtx, bool),
    RegionPar(Box<RegionParSubstitution>),
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
    /// remain compressed between entries. Resident callers lend an owned value;
    /// protobuf-region callers consume the matching retained value range without
    /// decoding a `Vec<Par>` or projecting the trie into a collection.
    PathMap(Box<PathMapSubstitution>),
    /// Rewrite only the escaped top-level segments of one canonical PathMap
    /// key; structural segments remain byte-identical.
    PathMapKey(Box<PathMapKeySubstitution>),
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
    PathMapKeyK(Box<PathMapKeySubstitution>),
    RegionParK(Box<RegionParSubstitution>),
    RegionComponentK(RegionComponentKont),
    SendK {
        cost_region: Option<models::rhoapi::CostRegion>,
        persistent: bool,
        locally_free: Vec<u8>,
        connective_used: bool,
        n_data: usize,
        shift: i32,
    },
    BindK {
        cost_region: Option<models::rhoapi::CostRegion>,
        remainder: Option<Var>,
        free_count: i32,
        n_patterns: usize,
    },
    ReceiveK {
        cost_region: Option<models::rhoapi::CostRegion>,
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
    preserve: bool,
    values: PathMapValueSource,
    completion: PathMapCompletion,
}

pub(crate) struct PathMapKeySubstitution {
    key: ProtobufRegion,
    cursor: CanonicalTopLevelSegmentCursor,
    output: Vec<u8>,
    form: CanonicalTopLevelPathForm,
    ctx: SubCtx,
}

enum PathMapValueSource {
    Resident,
    Regions {
        plan: ProtobufCutPlan,
        next: usize,
        end: usize,
    },
}

enum PathMapCompletion {
    Expr,
    Occurrence(Multiplicity),
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
sub_pop!(pop_canonical_key, CanonicalKey, Vec<u8>);
sub_pop!(pop_expr, Expr, PlannedComponentValue);
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
        PlannedSubVal::CanonicalKey(_) | PlannedSubVal::Bind(_) | PlannedSubVal::Case(_) => {
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
            #[cfg(feature = "protobuf-projection-experiment")]
            models::rust::par_construction::substitution_cost::record(
                models::rust::par_construction::substitution_cost::Event::VisitWork,
            );
            match w {
                SubWork::Par(slot, ctx) => {
                    let term = match slot {
                        Some(p) => p,
                        None => return Err(missing_required_field::<Par>()),
                    };
                    descend_par(term, ctx, view, empty, &mut work);
                }
                SubWork::ParRegion(region, ctx) => {
                    descend_par_region(region, ctx, view, empty, &mut work)?
                }
                SubWork::ImportParRegion(region) => {
                    descend_preserved_par_region(region, empty, &mut work)?;
                }
                SubWork::PlannedPar(par) => vals.push(PlannedSubVal::Par(par)),
                SubWork::CanonicalKey(key) => vals.push(PlannedSubVal::CanonicalKey(key)),
                SubWork::RegionComponent(occurrence, ctx, preserve) => descend_region_component(
                    occurrence,
                    ctx,
                    preserve,
                    view,
                    empty,
                    &mut construction,
                    &mut work,
                    &mut vals,
                )?,
                SubWork::RegionPar(state) => {
                    step_region_par(state, &mut construction, &mut work, &mut vals)?
                }
                SubWork::Expr(term, ctx) => descend_expr(term, ctx, view, &mut work)?,
                SubWork::Send(term, ctx) => descend_send(term, ctx, view, &mut work),
                SubWork::Receive(term, ctx) => descend_receive(term, ctx, view, &mut work),
                SubWork::New(term, ctx) => descend_new(term, ctx, view, &mut work),
                SubWork::Match(term, ctx) => descend_match(term, ctx, view, &mut work),
                SubWork::If(term, ctx) => descend_if(term, ctx, view, &mut work),
                SubWork::Bundle(term, ctx) => descend_bundle(term, ctx, &mut work),
                SubWork::Bind(term, ctx) => descend_bind(term, ctx, view, &mut work),
                SubWork::Case(term, ctx) => descend_case(term, ctx, view, &mut work),
                SubWork::PathMap(state) => {
                    step_pathmap(state, &mut construction, &mut work, &mut vals)?
                }
                SubWork::PathMapKey(state) => step_pathmap_key(state, &mut work, &mut vals)?,
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

fn append_resident_family<T: ParBagComponent>(
    term: &Par,
    family: EncodedFamily,
    occurrences: &mut Vec<EncodedOccurrence>,
) {
    term.visit_encoded::<T>(|entry| {
        occurrences.push(EncodedOccurrence {
            family,
            region: Some(ProtobufRegion::copy_from_slice(entry.payload())),
            canonical_ezipper: None,
            multiplicity: entry.multiplicity(),
        });
    });
}

fn append_resident_exprs(term: &Par, occurrences: &mut Vec<EncodedOccurrence>) {
    term.visit_encoded::<Expr>(|entry| {
        let multiplicity = entry.multiplicity();
        if let Some(canonical_ezipper) = entry.to_owned_canonical_ezipper() {
            occurrences.push(EncodedOccurrence {
                family: EncodedFamily::Expr,
                region: None,
                canonical_ezipper: Some(canonical_ezipper),
                multiplicity,
            });
        } else {
            occurrences.push(EncodedOccurrence {
                family: EncodedFamily::Expr,
                region: Some(ProtobufRegion::copy_from_slice(entry.payload())),
                canonical_ezipper: None,
                multiplicity,
            });
        }
    });
}

fn append_region_family(
    run: Option<&CanonicalRegionDelimitedRun>,
    family: EncodedFamily,
    occurrences: &mut Vec<EncodedOccurrence>,
) -> Result<(), InterpreterError> {
    let Some(run) = run else { return Ok(()) };
    let mut cursor = run.cursor();
    while let Some(region) = cursor
        .next_region()
        .map_err(|error| InterpreterError::DecodeError(error.to_string()))?
    {
        if let Some(previous) = occurrences.last_mut().filter(|previous| {
            previous.family == family
                && previous
                    .region
                    .as_ref()
                    .is_some_and(|previous| previous.as_bytes() == region.as_bytes())
        }) {
            previous.multiplicity = previous
                .multiplicity
                .checked_add(Multiplicity::ONE)
                .map_err(|error| InterpreterError::NormalizerError(error.to_string()))?;
        } else {
            occurrences.push(EncodedOccurrence {
                family,
                region: Some(region),
                canonical_ezipper: None,
                multiplicity: Multiplicity::ONE,
            });
        }
    }
    Ok(())
}

fn schedule_region_par(
    mut occurrences: Vec<EncodedOccurrence>,
    locally_free: Vec<u8>,
    connective_used: bool,
    ctx: SubCtx,
    preserve: bool,
    empty: PlannedPar,
    work: &mut Vec<SubWork>,
) {
    // `pop()` must yield the semantic/error order documented by `descend_par`.
    occurrences.reverse();
    work.push(SubWork::RegionPar(Box::new(RegionParSubstitution {
        rest: occurrences,
        acc: empty,
        locally_free,
        connective_used,
        ctx,
        preserve,
    })));
}

fn descend_par(
    term: Par,
    ctx: SubCtx,
    view: EnvView<'_>,
    empty: PlannedPar,
    work: &mut Vec<SubWork>,
) {
    let mut occurrences = Vec::with_capacity(term.distinct_len());

    // This is the legacy substitution/error order, not protobuf tag order.
    // Every visitor lends one distinct PathMap endpoint and its Bag count; the
    // protobuf body is copied once into the backing shared by all descendants.
    append_resident_exprs(&term, &mut occurrences);
    append_resident_family::<Connective>(&term, EncodedFamily::Connective, &mut occurrences);
    append_resident_family::<Send>(&term, EncodedFamily::Send, &mut occurrences);
    append_resident_family::<Bundle>(&term, EncodedFamily::Bundle, &mut occurrences);
    append_resident_family::<Receive>(&term, EncodedFamily::Receive, &mut occurrences);
    append_resident_family::<New>(&term, EncodedFamily::New, &mut occurrences);
    append_resident_family::<Match>(&term, EncodedFamily::Match, &mut occurrences);
    append_resident_family::<If>(&term, EncodedFamily::If, &mut occurrences);
    append_resident_family::<GUnforgeable>(&term, EncodedFamily::Unforgeable, &mut occurrences);
    append_resident_family::<CostTokenStack>(
        &term,
        EncodedFamily::CostTokenStack,
        &mut occurrences,
    );
    let locally_free = term.locally_free().to_vec();
    let connective_used = term.connective_used();

    schedule_region_par(
        occurrences,
        set_bits_until(locally_free, view.shift(ctx)),
        connective_used,
        ctx,
        false,
        empty,
        work,
    );
}

fn descend_par_region(
    region: ProtobufRegion,
    ctx: SubCtx,
    view: EnvView<'_>,
    empty: PlannedPar,
    work: &mut Vec<SubWork>,
) -> Result<(), InterpreterError> {
    let index = CanonicalParRegionIndex::parse(region)
        .map_err(|error| InterpreterError::DecodeError(error.to_string()))?;
    let mut occurrences = Vec::new();
    append_region_family(index.exprs.as_ref(), EncodedFamily::Expr, &mut occurrences)?;
    append_region_family(
        index.connectives.as_ref(),
        EncodedFamily::Connective,
        &mut occurrences,
    )?;
    append_region_family(index.sends.as_ref(), EncodedFamily::Send, &mut occurrences)?;
    append_region_family(
        index.bundles.as_ref(),
        EncodedFamily::Bundle,
        &mut occurrences,
    )?;
    append_region_family(
        index.receives.as_ref(),
        EncodedFamily::Receive,
        &mut occurrences,
    )?;
    append_region_family(index.news.as_ref(), EncodedFamily::New, &mut occurrences)?;
    append_region_family(
        index.matches.as_ref(),
        EncodedFamily::Match,
        &mut occurrences,
    )?;
    append_region_family(
        index.conditionals.as_ref(),
        EncodedFamily::If,
        &mut occurrences,
    )?;
    append_region_family(
        index.unforgeables.as_ref(),
        EncodedFamily::Unforgeable,
        &mut occurrences,
    )?;
    append_region_family(
        index.cost_token_stacks.as_ref(),
        EncodedFamily::CostTokenStack,
        &mut occurrences,
    )?;
    let locally_free = index
        .locally_free
        .as_ref()
        .map_or_else(Vec::new, |bytes| bytes.as_bytes().to_vec());
    schedule_region_par(
        occurrences,
        set_bits_until(locally_free, view.shift(ctx)),
        index.connective_used,
        ctx,
        false,
        empty,
        work,
    );
    Ok(())
}

fn descend_preserved_par_region(
    region: ProtobufRegion,
    empty: PlannedPar,
    work: &mut Vec<SubWork>,
) -> Result<(), InterpreterError> {
    // Identity import is reachable only through a `Par` cut descended from a
    // typed, validated resident RPB3 endpoint. `ProtobufRegion` supplies the
    // shared range, while that enclosing endpoint supplies semantic trust.
    // Preserve mode therefore retains canonical bytes; it is not a public
    // decoder for arbitrary canonically framed protobuf.
    let index = CanonicalParRegionIndex::parse(region)
        .map_err(|error| InterpreterError::DecodeError(error.to_string()))?;
    let mut occurrences = Vec::new();
    append_region_family(index.exprs.as_ref(), EncodedFamily::Expr, &mut occurrences)?;
    append_region_family(
        index.connectives.as_ref(),
        EncodedFamily::Connective,
        &mut occurrences,
    )?;
    append_region_family(index.sends.as_ref(), EncodedFamily::Send, &mut occurrences)?;
    append_region_family(
        index.bundles.as_ref(),
        EncodedFamily::Bundle,
        &mut occurrences,
    )?;
    append_region_family(
        index.receives.as_ref(),
        EncodedFamily::Receive,
        &mut occurrences,
    )?;
    append_region_family(index.news.as_ref(), EncodedFamily::New, &mut occurrences)?;
    append_region_family(
        index.matches.as_ref(),
        EncodedFamily::Match,
        &mut occurrences,
    )?;
    append_region_family(
        index.conditionals.as_ref(),
        EncodedFamily::If,
        &mut occurrences,
    )?;
    append_region_family(
        index.unforgeables.as_ref(),
        EncodedFamily::Unforgeable,
        &mut occurrences,
    )?;
    let locally_free = index
        .locally_free
        .as_ref()
        .map_or_else(Vec::new, |bytes| bytes.as_bytes().to_vec());
    schedule_region_par(
        occurrences,
        locally_free,
        index.connective_used,
        SubCtx::root(0),
        true,
        empty,
        work,
    );
    Ok(())
}

fn step_region_par(
    mut state: Box<RegionParSubstitution>,
    construction: &mut ParConstruction,
    work: &mut Vec<SubWork>,
    vals: &mut Vec<PlannedSubVal>,
) -> Result<(), InterpreterError> {
    if let Some(occurrence) = state.rest.pop() {
        let ctx = state.ctx;
        let preserve = state.preserve;
        work.push(SubWork::Combine(SubKont::RegionParK(state)));
        work.push(SubWork::RegionComponent(occurrence, ctx, preserve));
        return Ok(());
    }

    let root = if state.preserve {
        construction.with_metadata(state.acc, state.locally_free, state.connective_used)?
    } else {
        let metadata = construction.par(
            std::iter::empty(),
            state.locally_free,
            state.connective_used,
        )?;
        construction.parallel(state.acc, metadata)?
    };
    vals.push(PlannedSubVal::Par(root));
    Ok(())
}

struct RegionCutCursor<'a> {
    plan: &'a ProtobufCutPlan,
    at: usize,
}

impl<'a> RegionCutCursor<'a> {
    fn new(plan: &'a ProtobufCutPlan) -> Self { Self { plan, at: 0 } }

    fn take(&mut self) -> Result<ProtobufRegion, InterpreterError> {
        let region = self
            .plan
            .cuts()
            .get(self.at)
            .ok_or_else(|| {
                InterpreterError::DecodeError(
                    "protobuf region shell has more present Par slots than retained cuts"
                        .to_owned(),
                )
            })?
            .region()
            .clone();
        self.at += 1;
        Ok(region)
    }

    fn required(&mut self, present: bool, ctx: SubCtx) -> Result<RegionChild, InterpreterError> {
        if present {
            Ok(RegionChild::Substitute(self.take()?, ctx))
        } else {
            Ok(RegionChild::Missing(ctx))
        }
    }

    fn required_mode(
        &mut self,
        present: bool,
        ctx: SubCtx,
        preserve: bool,
    ) -> Result<RegionChild, InterpreterError> {
        if preserve {
            if present {
                self.preserved()
            } else {
                Ok(RegionChild::Missing(ctx))
            }
        } else {
            self.required(present, ctx)
        }
    }

    fn optional(
        &mut self,
        present: bool,
        ctx: SubCtx,
        empty: PlannedPar,
    ) -> Result<RegionChild, InterpreterError> {
        if present {
            Ok(RegionChild::Substitute(self.take()?, ctx))
        } else {
            Ok(RegionChild::Empty(empty))
        }
    }

    fn optional_mode(
        &mut self,
        present: bool,
        ctx: SubCtx,
        empty: PlannedPar,
        preserve: bool,
    ) -> Result<RegionChild, InterpreterError> {
        if preserve {
            if present {
                self.preserved()
            } else {
                Ok(RegionChild::Empty(empty))
            }
        } else {
            self.optional(present, ctx, empty)
        }
    }

    fn substituted(&mut self, ctx: SubCtx) -> Result<RegionChild, InterpreterError> {
        Ok(RegionChild::Substitute(self.take()?, ctx))
    }

    fn traversed(&mut self, ctx: SubCtx, preserve: bool) -> Result<RegionChild, InterpreterError> {
        if preserve {
            self.preserved()
        } else {
            self.substituted(ctx)
        }
    }

    fn preserved(&mut self) -> Result<RegionChild, InterpreterError> {
        Ok(RegionChild::Preserve(self.take()?))
    }

    fn finish(self) -> Result<(), InterpreterError> {
        if self.at == self.plan.cuts().len() {
            Ok(())
        } else {
            Err(InterpreterError::DecodeError(format!(
                "protobuf region shell consumed {} of {} retained Par slots",
                self.at,
                self.plan.cuts().len()
            )))
        }
    }
}

fn push_region_children(work: &mut Vec<SubWork>, children: Vec<RegionChild>) {
    for child in children.into_iter().rev() {
        work.push(match child {
            RegionChild::Substitute(region, ctx) => SubWork::ParRegion(region, ctx),
            RegionChild::Preserve(region) => SubWork::ImportParRegion(region),
            RegionChild::Missing(ctx) => SubWork::Par(None, ctx),
            RegionChild::Empty(par) => SubWork::PlannedPar(par),
        });
    }
}

fn schedule_region_component(
    shell: RegionComponentShell,
    multiplicity: Multiplicity,
    children: Vec<RegionChild>,
    wire_order: Vec<usize>,
    preserve: bool,
    work: &mut Vec<SubWork>,
) {
    let child_count = children.len();
    work.push(SubWork::Combine(SubKont::RegionComponentK(
        RegionComponentKont {
            shell,
            multiplicity,
            child_count,
            wire_order,
            preserve,
        },
    )));
    push_region_children(work, children);
}

fn component_contribution(
    construction: &mut ParConstruction,
    component: PlannedComponent,
    multiplicity: Multiplicity,
    locally_free: Vec<u8>,
    connective_used: bool,
) -> Result<PlannedPar, InterpreterError> {
    Ok(construction.par([(component, multiplicity)], locally_free, connective_used)?)
}

fn preserve_incomplete_region<T: ParBagComponent>(
    shell: &T,
    occurrence: &EncodedOccurrence,
    preserve: bool,
    construction: &mut ParConstruction,
    vals: &mut Vec<PlannedSubVal>,
) -> Result<bool, InterpreterError> {
    if !preserve {
        return Ok(false);
    }
    let region = occurrence.region.as_ref().expect("region occurrence");
    let Some(component) =
        construction.import_admitted_incomplete_component_region(shell, region.clone())?
    else {
        return Ok(false);
    };
    vals.push(PlannedSubVal::Par(component_contribution(
        construction,
        component,
        occurrence.multiplicity,
        Vec::new(),
        false,
    )?));
    Ok(true)
}

fn scale_contribution(
    construction: &mut ParConstruction,
    par: PlannedPar,
    multiplicity: Multiplicity,
) -> Result<PlannedPar, InterpreterError> {
    Ok(construction.scale_multiplicity(par, multiplicity)?)
}

fn region_decode_error(error: impl std::fmt::Display) -> InterpreterError {
    InterpreterError::DecodeError(error.to_string())
}

/// Recognize the opaque runtime-only zipper arm from the canonical `Expr`
/// envelope without first building a cut-at-`Par` plan for the PathMap stored
/// inside it. The byte-native importer derives its score and summaries from
/// the canonical envelope and skips the subtree that substitution is
/// forbidden to enter.
fn region_expr_is_ezipper(region: &ProtobufRegion) -> bool {
    canonical_expr_oneof_tag(region.as_bytes()) == Some(33)
}

fn begin_region_epathmap(
    pathmap: models::rhoapi::EPathMap,
    plan: ProtobufCutPlan,
    occurrence: EncodedOccurrence,
    ctx: SubCtx,
    preserve: bool,
    view: EnvView<'_>,
    work: &mut Vec<SubWork>,
) -> Result<(), InterpreterError> {
    let [group] = plan.epathmaps() else {
        return Err(InterpreterError::DecodeError(format!(
            "direct EPathMap Expr retained {} snapshot groups instead of one",
            plan.epathmaps().len()
        )));
    };
    let cut_range = group.cut_range();
    let parts = pathmap.into_owned_parts();
    let mode = match &parts.entries {
        OwnedEPathMapEntries::Empty => EPathMapMode::Empty,
        OwnedEPathMapEntries::Set(_) => EPathMapMode::Set,
        OwnedEPathMapEntries::Map(_) => EPathMapMode::Map,
    };
    if mode != group.mode() {
        return Err(InterpreterError::DecodeError(
            "shallow EPathMap mode disagrees with its retained ACT snapshot".to_owned(),
        ));
    }
    let result = match mode {
        EPathMapMode::Empty => EPathMapRepr::Empty,
        EPathMapMode::Set => EPathMapRepr::Set(PathMap::new()),
        EPathMapMode::Map => EPathMapRepr::Map(PathMap::new()),
    };
    work.push(SubWork::PathMap(Box::new(PathMapSubstitution {
        entries: parts.entries,
        result,
        locally_free: if preserve {
            parts.locally_free
        } else {
            set_bits_until(parts.locally_free, view.shift(ctx))
        },
        connective_used: parts.connective_used,
        remainder: parts.remainder,
        ctx,
        preserve,
        values: PathMapValueSource::Regions {
            plan,
            next: cut_range.start,
            end: cut_range.end,
        },
        completion: PathMapCompletion::Occurrence(occurrence.multiplicity),
    })));
    Ok(())
}

fn descend_region_component(
    mut occurrence: EncodedOccurrence,
    ctx: SubCtx,
    preserve: bool,
    view: EnvView<'_>,
    empty: PlannedPar,
    construction: &mut ParConstruction,
    work: &mut Vec<SubWork>,
    vals: &mut Vec<PlannedSubVal>,
) -> Result<(), InterpreterError> {
    if let Some(canonical) = occurrence.canonical_ezipper.take() {
        debug_assert_eq!(occurrence.family, EncodedFamily::Expr);
        let locally_free = canonical.locally_free().to_vec();
        let connective_used = canonical.connective_used();
        let component = construction.import_canonical_ezipper(canonical)?;
        vals.push(PlannedSubVal::Par(component_contribution(
            construction,
            component,
            occurrence.multiplicity,
            locally_free,
            connective_used,
        )?));
        return Ok(());
    }

    let region = occurrence
        .region
        .as_ref()
        .expect("non-resident-EZipper occurrence retains its protobuf region");
    if occurrence.family == EncodedFamily::Expr && region_expr_is_ezipper(region) {
        // EZipper is runtime cursor state and substitution has always treated
        // it as opaque, including its PathMap. Avoid `cut_at_par`: doing so
        // would traverse and retain every value region merely to discard the
        // plan. The region descends from RPB3 typed-key validation (see
        // `descend_preserved_par_region`), so derive the exact score from the
        // embedded EPM1 bytes and retain the original protobuf body without
        // decoding either structure.
        match construction.import_canonical_ezipper_region(region.as_bytes()) {
            Ok((component, locally_free, connective_used)) => {
                vals.push(PlannedSubVal::Par(component_contribution(
                    construction,
                    component,
                    occurrence.multiplicity,
                    locally_free,
                    connective_used,
                )?));
                return Ok(());
            }
            Err(
                models::rust::par_construction::ParConstructionError::InvalidCanonicalEzipperRegion,
            ) if preserve => {}
            Err(error) => return Err(error.into()),
        }
    }

    let program = match occurrence.family {
        EncodedFamily::Expr => &EXPR_RHO_MATCH_PROGRAM,
        EncodedFamily::Connective => &CONNECTIVE_RHO_MATCH_PROGRAM,
        EncodedFamily::Send => &SEND_RHO_MATCH_PROGRAM,
        EncodedFamily::Bundle => &BUNDLE_RHO_MATCH_PROGRAM,
        EncodedFamily::Receive => &RECEIVE_RHO_MATCH_PROGRAM,
        EncodedFamily::New => &NEW_RHO_MATCH_PROGRAM,
        EncodedFamily::Match => &MATCH_RHO_MATCH_PROGRAM,
        EncodedFamily::If => &IF_RHO_MATCH_PROGRAM,
        EncodedFamily::Unforgeable => &GUNFORGEABLE_RHO_MATCH_PROGRAM,
        EncodedFamily::CostTokenStack => &COSTTOKENSTACK_RHO_MATCH_PROGRAM,
    };
    let plan = cut_at_par(region.clone(), program).map_err(region_decode_error)?;
    let shallow = plan.shallow_bytes();

    match occurrence.family {
        EncodedFamily::Send => {
            let mut shell = protobuf_decoder::decode_send(shallow).map_err(region_decode_error)?;
            if preserve_incomplete_region(&shell, &occurrence, preserve, construction, vals)? {
                return Ok(());
            }
            if !preserve {
                shell.locally_free = set_bits_until(shell.locally_free, view.shift(ctx));
                substitute_cost_region(&mut shell.cost_region, ctx, view);
            }
            let mut cuts = RegionCutCursor::new(&plan);
            let mut children = Vec::with_capacity(shell.data.len() + 1);
            children.push(cuts.required_mode(shell.chan.is_some(), ctx, preserve)?);
            for _ in &shell.data {
                children.push(cuts.traversed(ctx, preserve)?);
            }
            cuts.finish()?;
            schedule_region_component(
                RegionComponentShell::Send(shell),
                occurrence.multiplicity,
                children,
                Vec::new(),
                preserve,
                work,
            );
        }
        EncodedFamily::Bundle => {
            let shell = protobuf_decoder::decode_bundle(shallow).map_err(region_decode_error)?;
            if preserve_incomplete_region(&shell, &occurrence, preserve, construction, vals)? {
                return Ok(());
            }
            let mut cuts = RegionCutCursor::new(&plan);
            let children = vec![cuts.required_mode(shell.body.is_some(), ctx, preserve)?];
            cuts.finish()?;
            schedule_region_component(
                RegionComponentShell::Bundle(shell),
                occurrence.multiplicity,
                children,
                Vec::new(),
                preserve,
                work,
            );
        }
        EncodedFamily::If => {
            let mut shell = protobuf_decoder::decode_if(shallow).map_err(region_decode_error)?;
            if preserve_incomplete_region(&shell, &occurrence, preserve, construction, vals)? {
                return Ok(());
            }
            if !preserve {
                shell.locally_free = set_bits_until(shell.locally_free, view.shift(ctx));
            }
            let mut cuts = RegionCutCursor::new(&plan);
            let children = vec![
                cuts.required_mode(shell.condition.is_some(), ctx, preserve)?,
                cuts.required_mode(shell.if_true.is_some(), ctx, preserve)?,
                cuts.required_mode(shell.if_false.is_some(), ctx, preserve)?,
            ];
            cuts.finish()?;
            schedule_region_component(
                RegionComponentShell::If(shell),
                occurrence.multiplicity,
                children,
                Vec::new(),
                preserve,
                work,
            );
        }
        EncodedFamily::New => {
            let mut shell = protobuf_decoder::decode_new(shallow).map_err(region_decode_error)?;
            if preserve_incomplete_region(&shell, &occurrence, preserve, construction, vals)? {
                return Ok(());
            }
            if !preserve {
                shell.locally_free = set_bits_until(shell.locally_free, view.shift(ctx));
            }
            let mut cuts = RegionCutCursor::new(&plan);
            let mut children = Vec::with_capacity(shell.injections.len() + 1);
            children.push(cuts.required_mode(
                shell.p.is_some(),
                ctx.shifted(shell.bind_count),
                preserve,
            )?);
            // Prost emits map entries by the BTreeMap's key order and the
            // generated region program visits the same wire order. Thus each
            // retained value cut remains associated with the corresponding
            // key in `shell.injections`; neither side projects or re-sorts the
            // map independently here.
            for _ in &shell.injections {
                children.push(cuts.preserved()?);
            }
            cuts.finish()?;
            schedule_region_component(
                RegionComponentShell::New(shell),
                occurrence.multiplicity,
                children,
                Vec::new(),
                preserve,
                work,
            );
        }
        EncodedFamily::Receive => {
            let mut shell =
                protobuf_decoder::decode_receive(shallow).map_err(region_decode_error)?;
            if preserve_incomplete_region(&shell, &occurrence, preserve, construction, vals)? {
                return Ok(());
            }
            if !preserve {
                shell.locally_free = set_bits_until(shell.locally_free, view.shift(ctx));
                substitute_cost_region(&mut shell.cost_region, ctx, view);
                for bind in &mut shell.binds {
                    substitute_cost_region(&mut bind.cost_region, ctx, view);
                }
            }
            let mut cuts = RegionCutCursor::new(&plan);
            let mut children = Vec::new();
            let mut wire_order = Vec::new();
            for bind in &shell.binds {
                let mut patterns = Vec::with_capacity(bind.patterns.len());
                for _ in &bind.patterns {
                    patterns.push(cuts.traversed(ctx.deeper(), preserve)?);
                }
                let source = cuts.required_mode(bind.source.is_some(), ctx, preserve)?;

                let source_index = children.len();
                children.push(source);
                let pattern_start = children.len();
                children.extend(patterns);
                wire_order.extend(pattern_start..children.len());
                wire_order.push(source_index);
            }
            let shifted = ctx.shifted(shell.bind_count);
            let body_index = children.len();
            children.push(cuts.required_mode(shell.body.is_some(), shifted, preserve)?);
            wire_order.push(body_index);
            let condition_index = children.len();
            children.push(cuts.optional_mode(
                shell.condition.is_some(),
                shifted,
                empty,
                preserve,
            )?);
            wire_order.push(condition_index);
            cuts.finish()?;
            schedule_region_component(
                RegionComponentShell::Receive(shell),
                occurrence.multiplicity,
                children,
                wire_order,
                preserve,
                work,
            );
        }
        EncodedFamily::Match => {
            let mut shell = protobuf_decoder::decode_match(shallow).map_err(region_decode_error)?;
            if preserve_incomplete_region(&shell, &occurrence, preserve, construction, vals)? {
                return Ok(());
            }
            if !preserve {
                shell.locally_free = set_bits_until(shell.locally_free, view.shift(ctx));
            }
            let mut cuts = RegionCutCursor::new(&plan);
            let mut children = Vec::new();
            let mut wire_order = Vec::new();

            let target_index = children.len();
            children.push(cuts.required_mode(shell.target.is_some(), ctx, preserve)?);
            wire_order.push(target_index);

            let mut retained_cases = Vec::with_capacity(shell.cases.len());
            for case in std::mem::take(&mut shell.cases) {
                let pattern = case.pattern.is_some().then(|| cuts.take()).transpose()?;
                let source = case.source.is_some().then(|| cuts.take()).transpose()?;
                let guard = case.guard.is_some().then(|| cuts.take()).transpose()?;
                if !preserve && (pattern.is_none() || source.is_none()) {
                    continue;
                }

                let shifted = ctx.shifted(case.free_count);
                let source_index = children.len();
                children.push(match source {
                    Some(region) if preserve => RegionChild::Preserve(region),
                    Some(region) => RegionChild::Substitute(region, shifted),
                    None => RegionChild::Missing(shifted),
                });
                let pattern_index = children.len();
                children.push(match pattern {
                    Some(region) if preserve => RegionChild::Preserve(region),
                    Some(region) => RegionChild::Substitute(region, ctx.deeper()),
                    None => RegionChild::Missing(ctx.deeper()),
                });
                let guard_index = children.len();
                children.push(match guard {
                    Some(region) if preserve => RegionChild::Preserve(region),
                    Some(region) => RegionChild::Substitute(region, shifted),
                    None => RegionChild::Empty(empty),
                });
                wire_order.extend([pattern_index, source_index, guard_index]);
                retained_cases.push(case);
            }
            shell.cases = retained_cases;
            cuts.finish()?;
            schedule_region_component(
                RegionComponentShell::Match(shell),
                occurrence.multiplicity,
                children,
                wire_order,
                preserve,
                work,
            );
        }
        EncodedFamily::Expr => {
            let expr = protobuf_decoder::decode_expr(shallow).map_err(region_decode_error)?;
            if preserve_incomplete_region(&expr, &occurrence, preserve, construction, vals)? {
                return Ok(());
            }
            if preserve && expr.expr_instance.is_none() {
                let component = construction.expr_component(&expr, &[], false)?;
                vals.push(PlannedSubVal::Par(component_contribution(
                    construction,
                    component,
                    occurrence.multiplicity,
                    Vec::new(),
                    false,
                )?));
                return Ok(());
            }
            let preserve_shell = preserve.then(|| expr.clone());
            let instance = super::unwrap_option_safe(expr.expr_instance)?;
            match instance {
                ExprInstance::EPathmapBody(pathmap) => {
                    return begin_region_epathmap(
                        pathmap, plan, occurrence, ctx, preserve, view, work,
                    );
                }
                ExprInstance::EZipperBody(_) => return Err(
                    models::rust::par_construction::ParConstructionError::InvalidCanonicalEzipperRegion.into()
                ),
                ExprInstance::EVarBody(evar) => {
                    let contribution = if preserve {
                        let shell = Expr {
                            expr_instance: Some(ExprInstance::EVarBody(evar)),
                        };
                        let component = construction.expr_component(&shell, &[], false)?;
                        component_contribution(
                            construction,
                            component,
                            occurrence.multiplicity,
                            expr_locally_free_ref(&shell, ctx.depth),
                            expr_connective_used_ref(&shell),
                        )?
                    } else {
                        match maybe_substitute_evar_view(evar, ctx, view)? {
                            Either::Left(evar) => {
                                let shell = Expr {
                                    expr_instance: Some(ExprInstance::EVarBody(evar)),
                                };
                                let component = construction.expr_component(&shell, &[], false)?;
                                component_contribution(
                                    construction,
                                    component,
                                    occurrence.multiplicity,
                                    expr_locally_free_ref(&shell, ctx.depth),
                                    expr_connective_used_ref(&shell),
                                )?
                            }
                            Either::Right(par) => {
                                let imported = construction.import_par(&par)?;
                                scale_contribution(construction, imported, occurrence.multiplicity)?
                            }
                        }
                    };
                    vals.push(PlannedSubVal::Par(contribution));
                    return Ok(());
                }
                ExprInstance::ESetBody(set) => {
                    let mut cuts = RegionCutCursor::new(&plan);
                    let mut children = Vec::with_capacity(set.ps.len());
                    for _ in &set.ps {
                        children.push(cuts.traversed(ctx, preserve)?);
                    }
                    cuts.finish()?;
                    schedule_region_component(
                        RegionComponentShell::Expr {
                            arm: ExprArm::ESet {
                                connective_used: set.connective_used,
                                locally_free: set.locally_free,
                                remainder: set.remainder,
                            },
                            preserve_shell,
                            depth: ctx.depth,
                            shift: view.shift(ctx),
                        },
                        occurrence.multiplicity,
                        children,
                        Vec::new(),
                        preserve,
                        work,
                    );
                }
                ExprInstance::EMapBody(map) => {
                    let mut cuts = RegionCutCursor::new(&plan);
                    let mut children = Vec::with_capacity(map.kvs.len() * 2);
                    for pair in &map.kvs {
                        children.push(cuts.required_mode(pair.key.is_some(), ctx, preserve)?);
                        children.push(cuts.required_mode(pair.value.is_some(), ctx, preserve)?);
                    }
                    cuts.finish()?;
                    schedule_region_component(
                        RegionComponentShell::Expr {
                            arm: ExprArm::EMap {
                                connective_used: map.connective_used,
                                locally_free: map.locally_free,
                                remainder: map.remainder,
                            },
                            preserve_shell,
                            depth: ctx.depth,
                            shift: view.shift(ctx),
                        },
                        occurrence.multiplicity,
                        children,
                        Vec::new(),
                        preserve,
                        work,
                    );
                }
                other => {
                    let (arm, slots) = split_expr_instance(other);
                    let pattern_slots = expr_arm_pattern_slots(&arm);
                    let mut cuts = RegionCutCursor::new(&plan);
                    let mut children = Vec::with_capacity(slots.len());
                    for (slot, child) in slots.into_iter().enumerate() {
                        let slot_ctx = if pattern_slots.contains(&slot) {
                            ctx.deeper()
                        } else {
                            ctx
                        };
                        children.push(cuts.required_mode(child.is_some(), slot_ctx, preserve)?);
                    }
                    cuts.finish()?;
                    schedule_region_component(
                        RegionComponentShell::Expr {
                            arm,
                            preserve_shell,
                            depth: ctx.depth,
                            shift: view.shift(ctx),
                        },
                        occurrence.multiplicity,
                        children,
                        Vec::new(),
                        preserve,
                        work,
                    );
                }
            }
        }
        EncodedFamily::Connective => {
            let connective =
                protobuf_decoder::decode_connective(shallow).map_err(region_decode_error)?;
            if preserve && connective.connective_instance.is_none() {
                let component = construction.connective_component(&connective, &[])?;
                vals.push(PlannedSubVal::Par(component_contribution(
                    construction,
                    component,
                    occurrence.multiplicity,
                    Vec::new(),
                    false,
                )?));
                return Ok(());
            }
            let Some(instance) = connective.connective_instance else {
                vals.push(PlannedSubVal::Par(empty));
                return Ok(());
            };
            let instance = match instance {
                ConnectiveInstance::VarRefBody(var_ref) => {
                    let contribution = if preserve {
                        let shell = Connective {
                            connective_instance: Some(ConnectiveInstance::VarRefBody(var_ref)),
                        };
                        let value = plan_connective(construction, &shell, &[], ctx.depth)?;
                        component_contribution(
                            construction,
                            value.component,
                            occurrence.multiplicity,
                            value.locally_free,
                            value.connective_used,
                        )?
                    } else {
                        match maybe_substitute_var_ref_view(var_ref, ctx, view)? {
                            Either::Left(_) => {
                                let shell = Connective {
                                    connective_instance: Some(ConnectiveInstance::VarRefBody(
                                        var_ref,
                                    )),
                                };
                                let value = plan_connective(construction, &shell, &[], ctx.depth)?;
                                component_contribution(
                                    construction,
                                    value.component,
                                    occurrence.multiplicity,
                                    value.locally_free,
                                    value.connective_used,
                                )?
                            }
                            Either::Right(par) => {
                                let imported = construction.import_par(&par)?;
                                scale_contribution(construction, imported, occurrence.multiplicity)?
                            }
                        }
                    };
                    vals.push(PlannedSubVal::Par(contribution));
                    return Ok(());
                }
                other => other,
            };

            let (arm, child_count) = match &instance {
                ConnectiveInstance::ConnAndBody(body) => (ConnArm::ConnAnd, body.ps.len()),
                ConnectiveInstance::ConnOrBody(body) => (ConnArm::ConnOr, body.ps.len()),
                ConnectiveInstance::ConnNotBody(_) => (ConnArm::ConnNot, 1),
                ground => (ConnArm::Ground(ground.clone()), 0),
            };
            let shell = rebuild_connective(arm, vec![Par::default(); child_count]);
            let mut cuts = RegionCutCursor::new(&plan);
            let mut children = Vec::with_capacity(child_count);
            for _ in 0..child_count {
                children.push(cuts.traversed(ctx, preserve)?);
            }
            cuts.finish()?;
            schedule_region_component(
                RegionComponentShell::Connective {
                    shell,
                    depth: ctx.depth,
                },
                occurrence.multiplicity,
                children,
                Vec::new(),
                preserve,
                work,
            );
        }
        EncodedFamily::Unforgeable => {
            let value =
                protobuf_decoder::decode_g_unforgeable(shallow).map_err(region_decode_error)?;
            if !plan.cuts().is_empty() {
                return Err(InterpreterError::DecodeError(
                    "GUnforgeable unexpectedly retained recursive Par cuts".to_owned(),
                ));
            }
            let component = construction.unforgeable_component(&value)?;
            vals.push(PlannedSubVal::Par(component_contribution(
                construction,
                component,
                occurrence.multiplicity,
                Vec::new(),
                false,
            )?));
        }
        EncodedFamily::CostTokenStack => {
            let mut value =
                protobuf_decoder::decode_cost_token_stack(shallow).map_err(region_decode_error)?;
            if !plan.cuts().is_empty() {
                return Err(InterpreterError::DecodeError(
                    "CostTokenStack unexpectedly retained recursive Par cuts".to_owned(),
                ));
            }
            if !preserve {
                for layer in &mut value.layers {
                    substitute_cost_signature(layer, ctx, view);
                }
            }
            let component = construction.cost_token_stack_component(&value, &[])?;
            vals.push(PlannedSubVal::Par(component_contribution(
                construction,
                component,
                occurrence.multiplicity,
                Vec::new(),
                false,
            )?));
        }
    }
    Ok(())
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
            preserve: false,
            values: PathMapValueSource::Resident,
            completion: PathMapCompletion::Expr,
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

fn pathmap_key_work(key: Vec<u8>, ctx: SubCtx) -> Result<SubWork, InterpreterError> {
    // `OwnedEPathMapEntries` lends keys only after EntryTrie ingestion has
    // established canonical semantics; the cursor rechecks framing, not that
    // already-proven semantic invariant.
    let cursor = CanonicalTopLevelSegmentCursor::from_validated_path(&key)
        .map_err(|error| InterpreterError::DecodeError(error.to_string()))?;
    if cursor.is_stable() {
        return Ok(SubWork::CanonicalKey(key));
    }
    let form = cursor.form();
    let capacity = key.len();
    Ok(SubWork::PathMapKey(Box::new(PathMapKeySubstitution {
        key: ProtobufRegion::from(key),
        cursor,
        output: Vec::with_capacity(capacity),
        form,
        ctx,
    })))
}

fn step_pathmap_key(
    mut state: Box<PathMapKeySubstitution>,
    work: &mut Vec<SubWork>,
    vals: &mut Vec<PlannedSubVal>,
) -> Result<(), InterpreterError> {
    while let Some(segment) = state.cursor.next_segment(state.key.as_bytes()) {
        match segment {
            CanonicalTopLevelSegment::Stable { envelope } => {
                state.output.extend_from_slice(
                    state
                        .key
                        .as_bytes()
                        .get(envelope)
                        .expect("the canonical key cursor produced an in-bounds segment"),
                );
            }
            CanonicalTopLevelSegment::Escape { payload, .. } => {
                let payload = state
                    .key
                    .subregion(CanonicalByteRange {
                        start: payload.start,
                        end: payload.end,
                    })
                    .expect("the canonical key cursor produced an in-bounds escape payload");
                let ctx = state.ctx;
                work.push(SubWork::Combine(SubKont::PathMapKeyK(state)));
                work.push(SubWork::ParRegion(payload, ctx));
                return Ok(());
            }
        }
    }

    if state.form == CanonicalTopLevelPathForm::Split {
        state.output.push(path_tag::TERM);
    }
    vals.push(PlannedSubVal::CanonicalKey(state.output));
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
            let ctx = state.ctx;
            let key_work = if state.preserve {
                SubWork::CanonicalKey(key)
            } else {
                pathmap_key_work(key, ctx)?
            };
            work.push(SubWork::Combine(SubKont::PathMapSetK(state)));
            work.push(key_work);
        }
        Some(OwnedEPathMapEntry::Map { key, value }) => {
            let ctx = state.ctx;
            let key_work = if state.preserve {
                SubWork::CanonicalKey(key)
            } else {
                pathmap_key_work(key, ctx)?
            };
            let value_work = match &mut state.values {
                PathMapValueSource::Resident => SubWork::Par(Some(value), ctx),
                PathMapValueSource::Regions { plan, next, end } => {
                    if *next >= *end {
                        return Err(InterpreterError::DecodeError(
                            "EPathMap topology exhausted its retained value-cut range".to_owned(),
                        ));
                    }
                    let cut = plan.cuts().get(*next).ok_or_else(|| {
                        InterpreterError::DecodeError(
                            "EPathMap topology has more map entries than retained value cuts"
                                .to_owned(),
                        )
                    })?;
                    *next += 1;
                    if state.preserve {
                        SubWork::ImportParRegion(cut.region().clone())
                    } else {
                        SubWork::ParRegion(cut.region().clone(), ctx)
                    }
                }
            };
            work.push(SubWork::Combine(SubKont::PathMapMapK(state)));
            // LIFO: key is substituted before its associated value.
            work.push(value_work);
            work.push(key_work);
        }
        None => {
            if let PathMapValueSource::Regions { next, end, .. } = &state.values {
                if next != end {
                    return Err(InterpreterError::DecodeError(format!(
                        "EPathMap topology consumed {next} of {end} retained value cuts"
                    )));
                }
            }
            let component = construction.epathmap_repr_expr_component(
                state.result,
                state.locally_free.clone(),
                state.connective_used,
                state.remainder,
            )?;
            let value = PlannedComponentValue {
                component,
                locally_free: state.locally_free,
                connective_used: state.connective_used,
            };
            match state.completion {
                PathMapCompletion::Expr => vals.push(PlannedSubVal::Expr(value)),
                PathMapCompletion::Occurrence(multiplicity) => {
                    vals.push(PlannedSubVal::Par(component_contribution(
                        construction,
                        value.component,
                        multiplicity,
                        value.locally_free,
                        value.connective_used,
                    )?));
                }
            }
        }
    }
    Ok(())
}

fn descend_send(mut term: Send, ctx: SubCtx, view: EnvView<'_>, work: &mut Vec<SubWork>) {
    substitute_cost_region(&mut term.cost_region, ctx, view);
    let Send {
        chan,
        data,
        persistent,
        locally_free,
        connective_used,
        cost_region,
    } = term;
    work.push(SubWork::Combine(SubKont::SendK {
        cost_region,
        persistent,
        locally_free,
        connective_used,
        n_data: data.len(),
        shift: view.shift(ctx),
    }));
    push_reversed(work, data, |p| SubWork::Par(Some(p), ctx));
    work.push(SubWork::Par(chan, ctx));
}

fn descend_bind(mut term: ReceiveBind, ctx: SubCtx, view: EnvView<'_>, work: &mut Vec<SubWork>) {
    substitute_cost_region(&mut term.cost_region, ctx, view);
    let ReceiveBind {
        patterns,
        source,
        remainder,
        free_count,
        cost_region,
    } = term;
    work.push(SubWork::Combine(SubKont::BindK {
        cost_region,
        remainder,
        free_count,
        n_patterns: patterns.len(),
    }));
    // Patterns are substituted at `depth + 1`; the source channel is not.
    let deeper = ctx.deeper();
    push_reversed(work, patterns, move |p| SubWork::Par(Some(p), deeper));
    work.push(SubWork::Par(source, ctx));
}

fn descend_receive(mut term: Receive, ctx: SubCtx, view: EnvView<'_>, work: &mut Vec<SubWork>) {
    substitute_cost_region(&mut term.cost_region, ctx, view);
    let Receive {
        binds,
        body,
        persistent,
        peek,
        bind_count,
        locally_free,
        connective_used,
        condition,
        cost_region,
    } = term;
    let has_condition = condition.is_some();
    work.push(SubWork::Combine(SubKont::ReceiveK {
        cost_region,
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
            let key = pop_canonical_key(vals);
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
            let key = pop_canonical_key(vals);
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
        SubKont::PathMapKeyK(mut state) => {
            let substituted = pop_par(vals);
            match state.form {
                CanonicalTopLevelPathForm::Bare => {
                    debug_assert!(state.output.is_empty());
                    construction.emit_canonical_path_into(substituted, &mut state.output)?;
                }
                CanonicalTopLevelPathForm::Split => {
                    construction.emit_canonical_segment_into(substituted, &mut state.output)?;
                }
            }
            work.push(SubWork::PathMapKey(state));
        }
        SubKont::RegionParK(mut state) => {
            let contribution = pop_par(vals);
            state.acc = construction.parallel(state.acc, contribution)?;
            work.push(SubWork::RegionPar(state));
        }
        SubKont::RegionComponentK(kont) => {
            let preserve = kont.preserve;
            let semantic_children = pop_n_par(vals, kont.child_count);
            let children = if kont.wire_order.is_empty() {
                semantic_children
            } else {
                kont.wire_order
                    .iter()
                    .map(|&index| {
                        semantic_children.get(index).copied().ok_or_else(|| {
                            InterpreterError::DecodeError(format!(
                                "region component wire slot {index} is outside {} semantic children",
                                semantic_children.len()
                            ))
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?
            };

            let (component, locally_free, connective_used) = match kont.shell {
                RegionComponentShell::Expr {
                    arm,
                    preserve_shell,
                    depth,
                    shift,
                } => {
                    if let Some(shell) = preserve_shell {
                        let mut children = children;
                        if matches!(arm, ExprArm::EMethod { .. }) {
                            children.rotate_left(1);
                        }
                        let component = construction.expr_component(&shell, &children, false)?;
                        (
                            component,
                            expr_locally_free_ref(&shell, depth),
                            expr_connective_used_ref(&shell),
                        )
                    } else {
                        let value = plan_expr(construction, arm, children, depth, shift)?;
                        (value.component, value.locally_free, value.connective_used)
                    }
                }
                RegionComponentShell::Connective { shell, depth } => {
                    if preserve {
                        let component = construction.connective_component(&shell, &children)?;
                        (
                            component,
                            connective_locally_free_ref(&shell, depth),
                            connective_connective_used_ref(&shell),
                        )
                    } else {
                        let value = plan_connective(construction, &shell, &children, depth)?;
                        (value.component, value.locally_free, value.connective_used)
                    }
                }
                RegionComponentShell::Send(shell) => {
                    let component = construction.send_component(&shell, &children)?;
                    (component, Vec::new(), false)
                }
                RegionComponentShell::Bundle(mut shell) => {
                    let mut body = children[0];
                    if !preserve {
                        if let Some((inner, write_flag, read_flag)) =
                            construction.single_bundle(body)
                        {
                            shell.write_flag &= write_flag;
                            shell.read_flag &= read_flag;
                            body = inner;
                        }
                    }
                    shell.body = Some(Par::default());
                    let component = construction.bundle_component(&shell, &[body])?;
                    (component, Vec::new(), false)
                }
                RegionComponentShell::Receive(shell) => {
                    let component = construction.receive_component(&shell, &children)?;
                    (component, Vec::new(), false)
                }
                RegionComponentShell::New(shell) => {
                    let component = construction.new_component(&shell, &children)?;
                    (component, Vec::new(), false)
                }
                RegionComponentShell::Match(shell) => {
                    let component = construction.match_component(&shell, &children)?;
                    (component, Vec::new(), false)
                }
                RegionComponentShell::If(shell) => {
                    let component = construction.if_component(&shell, &children)?;
                    (component, Vec::new(), false)
                }
            };
            vals.push(PlannedSubVal::Par(component_contribution(
                construction,
                component,
                kont.multiplicity,
                locally_free,
                connective_used,
            )?));
        }
        SubKont::SendK {
            cost_region,
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
                cost_region,
            };
            let children: Vec<_> = std::iter::once(chan).chain(data).collect();
            vals.push(PlannedSubVal::Send(
                construction.send_component(&shell, &children)?,
            ));
        }
        SubKont::BindK {
            cost_region,
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
                cost_region,
            };
            let children = patterns
                .into_iter()
                .chain(std::iter::once(source))
                .collect();
            vals.push(PlannedSubVal::Bind(PlannedBind { shell, children }));
        }
        SubKont::ReceiveK {
            cost_region,
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
                cost_region,
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
#[path = "../../../tests/support/substitution_region_slots.rs"]
mod region_slot_tests;

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
