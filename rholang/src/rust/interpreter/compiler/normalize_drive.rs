//! # The normalizer as an explicit pushdown machine
//!
//! `normalize_ann_proc` and the 25 functions it is mutually recursive with form
//! **one strongly connected component** whose call depth is Θ(*source* nesting).
//! Measured at **43,542 bytes of native stack per bracket level** in debug and
//! **7,261** in release, that made a **577-byte** program —
//! `[`×288 · `0` · `]`×288 — abort a **release** node outright:
//!
//! ```text
//!    ┌──── 288 × '[' ────┐   ┌ 1 ┐   ┌──── 288 × ']' ────┐
//!    [ [ [ [ … [ [ [ [ [       0     ] ] ] ] ] … ] ] ] ]
//!    └──────────────────────── 577 bytes ────────────────────────┘
//! ```
//!
//! ## ★ Why this member was categorically worse than the rest of its family
//!
//! Four reasons, each verified against the code rather than argued
//! (`docs/design/audits/theta-depth-traversals-2026-07-26.md` §12.3):
//!
//! 1. **It fires before metering exists.** `InterpreterImpl::inj_attempt` runs
//!    `Compiler::source_to_adt_with_normalizer_env` as its *first* phase
//!    (`build-normalized-term`); the budget is not established until the next
//!    phase (`set-initial-cost`). There is no term to charge for and no budget
//!    to charge against, so **cost accounting could not bound this input** — not
//!    because the charge was too small, but because the charge did not exist yet.
//! 2. **The failure was not an `Err`.** The call site's
//!    `Err(e) => handle_error(InterpreterError::ParserError(..))` arm exists
//!    precisely to turn a bad deploy into a *failed deploy*; a stack overflow is
//!    a `SIGSEGV` on the guard page, is not unwindable, and is not an `Err`, so
//!    that arm **never ran** and the node process terminated instead.
//! 3. **Validators normalize deploys they receive.**
//!    `ReplayRuntimeOps::run_user_deploy` → `runtime_ops.evaluate(&deploy)` →
//!    `inj_attempt` → here, on source that arrived from the network. Producing
//!    the input requires no privilege and no stake.
//! 4. **The overflow was in the normalizer, not the parser** — the `gdb`
//!    backtrace at the fault is a clean repetition of `normalize_ann_proc →
//!    normalize_p_collect → normalize_collection → fold_match`, leaf inside
//!    `models::rust::utils::new_gint_expr`. The parser had already returned.
//!
//! ## Why the traversal was converted rather than depth-limited
//!
//! A pre-normalization depth guard was considered and rejected. It is
//! **protocol-visible** — it changes which deploys a node accepts — and it
//! cannot be set profile-independently: the measured ceiling is 287 source
//! levels in release and 45 in debug, so any single constant is either inert in
//! release or newly restrictive there. Converting the traversal removes the
//! ceiling instead of choosing where to put it. This is the same disposition
//! [§7.3] records for the `prost` `RECURSION_LIMIT` ("no *new* protocol-level
//! nesting cap").
//!
//! ## ★ What makes this conversion harder than the five that preceded it
//!
//! The five converted before it (`substitute`, `ParSortMatcher`,
//! `rho-pure-eval::eval_with`, the bincode decoder, `FoldMatch::free_check`)
//! are post-order folds: every child is independent of its siblings, so a
//! driver can push all children at once and reassemble on the way up. The
//! normalizer's state is **threaded**:
//!
//! | carrier | how it flows | what a naive post-order gets wrong |
//! |---|---|---|
//! | `ProcVisitInputs::free_map` | **left-to-right across siblings** — each sibling is normalized against what its predecessors bound | de Bruijn *levels*. A sibling that binds `n` names shifts every later sibling's free indices by `n`. The failure is **silent**: plausible-but-wrong indices, not a crash. |
//! | `ProcVisitInputs::bound_map_chain` | **scoped at binders** — cloned and pushed on entry to a binding form, popped on exit | the push/pop must become an explicit discipline rather than stack unwinding |
//! | `ProcVisitInputs::par` | **accumulates** — several arms build the result on the way *down* (`prepend_expr(input_par, …)`), not only on the way up | a post-order `Combine` alone cannot reassemble it |
//!
//! So each child's **input** depends on the previous child's **output**, and a
//! machine for this shape cannot schedule siblings in a batch. It schedules
//! **one child at a time**, and the continuation that receives child `i`'s
//! output is the thing that computes child `i+1`'s input. Every continuation in
//! [`NormKont`] is therefore a *frame*: it holds exactly the locals the
//! recursive function held across its recursive call.
//!
//! ## The machine
//!
//! ```text
//!    work (LIFO)                                 vals (LIFO)
//!    ┌────────────────────────┐                  ┌──────────────────┐
//!    │ Proc(child_i, input_i) │ ── descend ──▶   │                  │
//!    ├────────────────────────┤                  │  at most ONE     │
//!    │ Combine(K, filled=i)   │ ◀─ pushed FIRST  │  value is ever   │
//!    ├────────────────────────┤                  │  resident        │
//!    │ Combine(K', …)         │                  │                  │
//!    │ …                      │                  └──────────────────┘
//!    └────────────────────────┘
//!
//!    Combine(K) pops the value, folds it into K's accumulators, and then
//!      · if K still owes children:  pushes Combine(K with filled+1)
//!                                   then Descend(child_{filled+1})
//!      · if K is satisfied:         produces K's value
//! ```
//!
//! Three [`Step`] shapes cover every arm:
//!
//! * [`Step::Done`] — the node is complete; publish its value.
//! * [`Step::Descend`] — push one continuation, then one child.
//! * [`Step::Tail`] — **replace** the current obligation. This is the
//!   *desugarings* (`let`, `!?`, multi-receipt `for`, complex-source `for`, and
//!   both `recognize_*` lowerings), which end in an unconditional
//!   `normalize_ann_proc(&rewritten, input, …)`. They are proper tail calls, so
//!   they consume no continuation at all and a chain of them is flat.
//!
//! ## ★ The deficit invariant, and why it takes a different form here
//!
//! `sort_drive` maintains
//!
//! ```text
//!     |V| + D + |C| − Σ arity(k) == 1
//! ```
//!
//! with `arity(k)` the number of values `k` pops. That statement holds here too
//! — **and it degenerates**, because every continuation in this machine pops
//! exactly one value (the child that just finished), so `Σ arity = |C|` and the
//! law reduces to
//!
//! ```text
//!     |V| + D == 1                                      (the CONSERVATION LAW)
//! ```
//!
//! which is what [`norm_drive`] asserts at the head of every iteration. That is
//! not a weaker claim, it is the *same* claim specialised: a batch machine
//! spreads a node's `n` obligations across the work stack, where they are
//! visible to a pop-count; a threaded machine holds `n−1` of them *inside* the
//! continuation, where a pop-count cannot see them. Recording this rather than
//! transcribing `sort_drive`'s form matters — an invariant that cannot fail is
//! worth nothing, and one that fires spuriously is worse than none.
//!
//! The obligations the pop-count cannot see are therefore checked directly, and
//! this is where the cross-checking strength of the original invariant is
//! recovered:
//!
//! ```text
//!     ∀ k on the work stack:   filled(k) <  arity(k)          (SLOT INVARIANT)
//!     when k runs:             filled(k) += 1, and
//!                              filled(k) == arity(k)  ⟺  k produced a value
//! ```
//!
//! * [`NormKont::arity`] is an **independently spelled** exhaustive `match`
//!   giving the total number of child values each continuation's chain
//!   consumes, read off the *node's own shape* (`elements.len()`, `2`, `3`,
//!   `1 + args.len()`, `Σ|group| + |sources| + guard? + 1`, …). It deliberately
//!   duplicates what the arms do; the point is the disagreement.
//! * [`NormKont::filled`] is read off the continuation's **accumulators** — the
//!   vectors and `Option` slots the arms actually maintain.
//!
//! A `descend_*` that forgets a child, a `combine_*` that folds into the wrong
//! slot, or an arity that drifts from its arm therefore fires on the **first
//! malformed configuration on any term**, where a differential fires only if the
//! corpus happens to contain the witness. Both counters are maintained
//! **incrementally**: an `O(n)` rescan of `work` would make debug `O(n²)` on the
//! depth-4,096 terms `rholang/tests/stack_depth_gate.rs` feeds this machine.
//!
//! ## Teardown
//!
//! `Drop` for `Par` is itself a Θ(depth) recursive traversal (audit §5 row 10),
//! so an error path that simply let `work` and `vals` fall out of scope would
//! re-introduce the very class this conversion removes — on the *error* path,
//! which is exactly where a hostile input lands. [`norm_drive`] therefore hands
//! everything it still owns to [`models::rust::rholang::par_children::dismantle_all`]
//! before returning an `Err`. This is the same defect
//! `b98fa20a` found and fixed for `substitute` (audit §12.5, vacuity instance
//! #4).
//!
//! ## Neutrality
//!
//! `normalize` runs **before** a budget exists, so — unlike `substitute` — there
//! is no charge trace to preserve and no `reserve_*` call anywhere in the SCC.
//! The whole neutrality obligation is therefore *result* equality, and the
//! result is consensus-observable in full: this SCC decides free-variable
//! numbering and the normalized `Par` that `ParSortMatcher::sort_match` then
//! canonicalises and `sig.rs` signs. It is discharged by
//! `super::normalize_recursive` (the verbatim recursive twin) plus the
//! differential in `super::normalize_differential`, which compares the encoded
//! `Par` **bytes**, the final `free_map` and the final `bound_map_chain`.

use std::collections::HashMap;

use models::rhoapi::{Par, Var as ModelsVar};
use models::rust::rholang::par_children::dismantle_all;
use rholang_parser::ast::{
    AnnProc, BundleType, Case, KeyValuePair, Name, Names, ProcList, Signature, Var,
};
use rholang_parser::{RholangParser, SourceSpan};

use super::bound_map_chain::BoundMapChain;
use super::exports::{FreeMap, NameVisitInputs, NameVisitOutputs, ProcVisitInputs, ProcVisitOutputs};
use super::normalize::VarSort;
use super::normalizer::cost_accounting::ir::Sig;
use super::utils::{BinaryExpr, UnaryExpr};
use crate::rust::interpreter::errors::InterpreterError;

// ===========================================================================
// values
// ===========================================================================

/// A value produced by the machine, tagged by the entry point that produced it.
///
/// The producing [`NormWork`] determines the variant, so a mismatch on the value
/// stack is a driver bug and never an input error — which is why the `pop_*`
/// helpers below `unreachable!` rather than return an error.
pub(crate) enum NormVal {
    /// From `normalize_ann_proc`.
    Proc(ProcVisitOutputs),
    /// From `normalize_name`.
    Name(NameVisitOutputs),
    /// From `canon_quote` — the wire encoding of the sort-canonicalised `Par`.
    Bytes(Vec<u8>),
    /// From `signature_to_ir`.
    Sig(Sig),
}

impl NormVal {
    pub(crate) fn into_proc(self) -> ProcVisitOutputs {
        match self {
            NormVal::Proc(p) => p,
            _ => unreachable!("norm_drive: expected a Proc value on the value stack"),
        }
    }

    pub(crate) fn into_name(self) -> NameVisitOutputs {
        match self {
            NormVal::Name(n) => n,
            _ => unreachable!("norm_drive: expected a Name value on the value stack"),
        }
    }

    pub(crate) fn into_bytes(self) -> Vec<u8> {
        match self {
            NormVal::Bytes(b) => b,
            _ => unreachable!("norm_drive: expected a Bytes value on the value stack"),
        }
    }

    pub(crate) fn into_sig(self) -> Sig {
        match self {
            NormVal::Sig(s) => s,
            _ => unreachable!("norm_drive: expected a Sig value on the value stack"),
        }
    }

    /// Every `Par` this value owns, for iterative teardown on the error path.
    fn into_pars(self) -> Vec<Par> {
        match self {
            NormVal::Proc(p) => vec![p.par],
            NormVal::Name(n) => vec![n.par],
            NormVal::Bytes(_) | NormVal::Sig(_) => Vec::new(),
        }
    }
}

// ===========================================================================
// work
// ===========================================================================

/// A surface signature, borrowed from the arena where possible and owned where
/// the producer already owns it.
///
/// `Signature` derives `Clone`, and that derived `Clone` is itself recursive
/// over `Box<Signature>`. Cloning to get an owned copy would therefore *add* a
/// Θ(depth) traversal on a second type while removing one here, so the two
/// producers are distinguished instead: `recognize_signed_term` holds
/// `&'ast Signature<'ast>` (into the arena) and `desugar::strip_signed_binds`
/// hands back a `Vec<Signature<'ast>>` it already owns. Descending an `Own`
/// compound **moves** out of its boxes; no clone happens on either path.
pub(crate) enum SigInput<'ast> {
    Ref(&'ast Signature<'ast>),
    Own(Signature<'ast>),
}

/// A pending unit of work.
pub(crate) enum NormWork<'ast> {
    /// `normalize_ann_proc(proc, input, …)`.
    Proc {
        proc: AnnProc<'ast>,
        input: ProcVisitInputs,
    },
    /// `normalize_name(name, input, …)`.
    Name {
        name: Name<'ast>,
        input: NameVisitInputs,
    },
    /// `canon_quote(proc, …)` — normalize standalone at de Bruijn depth 0, then
    /// sort-canonicalise and encode. Produces [`NormVal::Bytes`].
    CanonQuote { proc: AnnProc<'ast> },
    /// `signature_to_ir(sig, bound_map_chain, …)`. Produces [`NormVal::Sig`].
    Sig {
        sig: SigInput<'ast>,
        bound_map_chain: BoundMapChain<VarSort>,
    },
    /// Run a continuation against the value on top of the value stack.
    Combine(NormKont<'ast>),
}

impl NormWork<'_> {
    /// Every `Par` this obligation owns, for iterative teardown on the error
    /// path.
    fn into_pars(self) -> Vec<Par> {
        match self {
            NormWork::Proc { input, .. } => vec![input.par],
            NormWork::Name { .. } | NormWork::CanonQuote { .. } | NormWork::Sig { .. } => Vec::new(),
            NormWork::Combine(k) => k.into_pars(),
        }
    }
}

// ===========================================================================
// continuations
// ===========================================================================

/// Which of `normalize_p_if`'s three children is next.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum IfPhase {
    /// The condition has not been normalized yet.
    Condition,
    /// The condition is in; `if_true` is next.
    TrueCase,
    /// Both are in; `if_false` (or a synthesised `Nil`) is next.
    FalseCase,
}

/// Which slot of the current `match` case is next.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum MatchPhase {
    /// The scrutinee has not been normalized yet.
    Target,
    /// The next value is a case *pattern*.
    Pattern,
    /// The next value is a case `where` *guard*.
    Guard,
    /// The next value is a case *body*.
    Body,
}

/// Which slot of a `for`-comprehension is next.
///
/// The order reproduces `normalize_p_input` exactly: **all** pattern groups,
/// then **all** sources, then the optional `where` guard, then the body.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum InputPhase {
    /// Name `name_idx` of pattern group `group_idx`.
    Patterns { group_idx: usize, name_idx: usize },
    /// Source name `idx`.
    Sources { idx: usize },
    /// The `where` guard.
    Guard,
    /// The continuation body.
    Body,
}

/// The four collection constructors of `normalize_collection`, as data.
///
/// The recursive form built these as closures capturing `optional_remainder`;
/// a closure cannot travel on a work stack, so the capture becomes a field and
/// the closure body becomes
/// [`super::normalizer::collection_normalize_matcher::build_collection_expr`].
pub(crate) enum CollectKind {
    List { remainder: Option<ModelsVar> },
    Tuple,
    Set { remainder: Option<ModelsVar> },
    PathMap { remainder: Option<ModelsVar> },
}

/// `normalize_p_input`'s frame. Boxed: it is by far the widest continuation and
/// `NormKont` is stored by value in a `Vec`.
pub(crate) struct InputK<'ast> {
    // ---- the shape, fixed at descend time -------------------------------
    /// One entry per receive bind: its formal names (borrowed from the parser
    /// arena, so the cost-syntax guard can walk them) and its remainder.
    pub patterns: Vec<(&'ast [Name<'ast>], Option<Var<'ast>>)>,
    /// One channel per receive bind, in the same order as `patterns`.
    pub sources: Vec<Name<'ast>>,
    pub guard: Option<AnnProc<'ast>>,
    pub body: AnnProc<'ast>,
    pub persistent: bool,
    pub peek: bool,
    pub input: ProcVisitInputs,

    // ---- where we are ---------------------------------------------------
    pub phase: InputPhase,

    // ---- pattern accumulators (per group, reset at each group) ----------
    pub group_pars: Vec<Par>,
    pub group_free: FreeMap<VarSort>,
    pub group_locally_free: Vec<u8>,
    /// `process_patterns`' output, one entry per completed group.
    pub done_patterns: Vec<(Vec<Par>, Option<ModelsVar>, FreeMap<VarSort>, Vec<u8>)>,

    // ---- source accumulators --------------------------------------------
    pub source_pars: Vec<Par>,
    pub source_free: FreeMap<VarSort>,
    pub source_locally_free: Vec<u8>,
    pub source_connective_used: bool,

    // ---- computed once, at the Sources → Guard/Body transition ----------
    pub body_env: BoundMapChain<VarSort>,
    pub receive_binds: Vec<models::rhoapi::ReceiveBind>,
    pub bind_count: usize,
    pub patterns_locally_free: Vec<u8>,
    pub guard_out: Option<ProcVisitOutputs>,
}

/// `normalize_p_match`'s frame. Boxed for the same reason as [`InputK`].
pub(crate) struct MatchK<'ast> {
    pub cases: &'ast [Case<'ast>],
    pub case_idx: usize,
    pub phase: MatchPhase,
    pub input: ProcVisitInputs,

    pub target: Option<ProcVisitOutputs>,
    /// `init_acc.0` — built with `insert(0, …)` and reversed at the end, exactly
    /// as the recursive form does.
    pub acc_cases: Vec<models::rhoapi::MatchCase>,
    /// `init_acc.1` — the free map threaded across cases.
    pub known_free: FreeMap<VarSort>,
    /// `init_acc.2`.
    pub acc_locally_free: Vec<u8>,
    /// `init_acc.3`.
    pub acc_connective_used: bool,

    // ---- per-case slots -------------------------------------------------
    pub case_env: BoundMapChain<VarSort>,
    pub bound_count: usize,
    pub pattern_par: Option<Par>,
    pub pattern_locally_free: Vec<u8>,
    pub guard_out: Option<ProcVisitOutputs>,
}

/// `normalize_p_send`'s frame.
pub(crate) struct SendK<'ast> {
    pub inputs: &'ast ProcList<'ast>,
    /// `0` means "the channel name has not been normalized yet"; `i ≥ 1` means
    /// `inputs[i − 1]` is next.
    pub idx: usize,
    pub name_par: Option<Par>,
    pub acc_pars: Vec<Par>,
    pub acc_locally_free: Vec<u8>,
    pub acc_connective_used: bool,
    pub free_map: FreeMap<VarSort>,
    pub bound_map_chain: BoundMapChain<VarSort>,
    pub input_par: Par,
    pub input_depth: i32,
    pub persistent: bool,
}

/// `normalize_p_method`'s frame.
///
/// ⚠ The recursive form folds the arguments **right to left**
/// (`args.iter().rev().try_fold(..)`) while `insert(0, …)`-ing each result, so
/// the *argument list* ends up in source order but the *free map* threads from
/// the last argument to the first. Both are reproduced: `arg_order` is the
/// reversed index sequence and `acc_args` is built with `insert(0, …)`.
pub(crate) struct MethodK<'ast> {
    pub args: &'ast ProcList<'ast>,
    /// `0` means "the receiver has not been normalized yet"; `i ≥ 1` means the
    /// `i`-th argument *in reverse source order* is next.
    pub idx: usize,
    pub method_name: String,
    pub target: Option<Par>,
    pub acc_args: Vec<Par>,
    pub acc_locally_free: Vec<u8>,
    pub acc_connective_used: bool,
    pub free_map: FreeMap<VarSort>,
    pub bound_map_chain: BoundMapChain<VarSort>,
    pub input_par: Par,
    pub input_depth: i32,
}

/// `normalize_p_contr`'s frame.
pub(crate) struct ContrK<'ast> {
    pub formals: &'ast Names<'ast>,
    /// `0` = the contract name; `1..=formals.names.len()` = formal `idx − 1`;
    /// `formals.names.len() + 1` = the body.
    pub idx: usize,
    pub name_out: Option<NameVisitOutputs>,
    pub acc_patterns: Vec<Par>,
    pub acc_free: FreeMap<VarSort>,
    pub acc_locally_free: Vec<u8>,
    pub remainder: Option<ModelsVar>,
    pub bound_count: usize,
    pub body: AnnProc<'ast>,
    pub input: ProcVisitInputs,
}

/// The post-order continuation: the frame a recursive call would have left on
/// the native stack, made explicit.
///
/// Every variant holds *exactly* the locals its recursive counterpart held
/// across its recursive call — never a child value that is still in flight,
/// which lives on the value stack.
pub(crate) enum NormKont<'ast> {
    // ---- normalize.rs ---------------------------------------------------
    /// `normalize_ann_proc::unary_exp`.
    Unary {
        input_par: Par,
        input_depth: i32,
        ctor: Box<dyn UnaryExpr>,
    },
    /// `normalize_ann_proc::binary_exp`.
    Binary {
        right: AnnProc<'ast>,
        input_par: Par,
        input_depth: i32,
        bound_map_chain: BoundMapChain<VarSort>,
        ctor: Box<dyn BinaryExpr>,
        left_par: Option<Par>,
    },

    // ---- one file per node type ------------------------------------------
    /// `normalize_p_par` — the flattened `|` sequence. `idx` counts operands
    /// **absorbed**, not the next one to schedule; [`NormKont::filled`] reads it
    /// verbatim, so the two must mean the same thing.
    ParSeq {
        procs: Vec<AnnProc<'ast>>,
        idx: usize,
        bound_map_chain: BoundMapChain<VarSort>,
    },
    /// `normalize_collection`'s `fold_match`, fused with `normalize_p_collect`.
    Collect {
        kind: CollectKind,
        elements: &'ast [AnnProc<'ast>],
        idx: usize,
        acc_pars: Vec<Par>,
        locally_free: Vec<u8>,
        connective_used: bool,
        known_free: FreeMap<VarSort>,
        bound_map_chain: BoundMapChain<VarSort>,
        input_par: Par,
        input_depth: i32,
    },
    /// `normalize_collection`'s `fold_match_map`, fused with
    /// `normalize_p_collect`. Two children per pair: key, then value.
    CollectMap {
        remainder: Option<ModelsVar>,
        pairs: &'ast [KeyValuePair<'ast>],
        idx: usize,
        /// `false` = the next value is a key, `true` = it is a value.
        on_value: bool,
        key_par: Option<Par>,
        acc_pairs: Vec<(Par, Par)>,
        locally_free: Vec<u8>,
        connective_used: bool,
        known_free: FreeMap<VarSort>,
        bound_map_chain: BoundMapChain<VarSort>,
        input_par: Par,
        input_depth: i32,
    },
    /// `normalize_p_conjunction`.
    Conjunction {
        right: AnnProc<'ast>,
        input: ProcVisitInputs,
        span: SourceSpan,
        left_par: Option<Par>,
    },
    /// `normalize_p_disjunction`.
    Disjunction {
        right: AnnProc<'ast>,
        input: ProcVisitInputs,
        span: SourceSpan,
        left_par: Option<Par>,
    },
    /// `normalize_p_matches`.
    Matches {
        right: AnnProc<'ast>,
        input: ProcVisitInputs,
        left: Option<ProcVisitOutputs>,
    },
    /// `normalize_p_negation`.
    Negation {
        input: ProcVisitInputs,
        span: SourceSpan,
    },
    /// `normalize_p_eval`.
    Eval { input_par: Par },
    /// `normalize_p_send`.
    Send(Box<SendK<'ast>>),
    /// `normalize_p_method`.
    Method(Box<MethodK<'ast>>),
    /// `normalize_p_if`, fused with `normalize.rs`'s `IfThenElse` wrapper —
    /// which normalizes against an empty `par` and appends the original
    /// afterwards, so `outer_par` carries it across.
    If {
        if_true: AnnProc<'ast>,
        if_false: Option<AnnProc<'ast>>,
        bound_map_chain: BoundMapChain<VarSort>,
        outer_par: Par,
        phase: IfPhase,
        condition: Option<ProcVisitOutputs>,
        true_case: Option<ProcVisitOutputs>,
    },
    /// `normalize_p_match`.
    Match(Box<MatchK<'ast>>),
    /// `normalize_p_new`.
    New {
        new_count: usize,
        uris: Vec<String>,
        input_par: Par,
    },
    /// `normalize_p_contr`.
    Contr(Box<ContrK<'ast>>),
    /// `normalize_p_bundle`.
    Bundle {
        bundle_type: BundleType,
        span: SourceSpan,
        input: ProcVisitInputs,
    },
    /// `normalize_p_input`.
    Input(Box<InputK<'ast>>),

    // ---- names -----------------------------------------------------------
    /// `normalize_name`'s `Name::Quote` arm — retypes a `ProcVisitOutputs` as a
    /// `NameVisitOutputs`.
    NameQuote,

    // ---- cost-accounted surface syntax -----------------------------------
    /// `canon_quote` — sort-canonicalise and encode the normalized `Par`.
    CanonQuote,
    /// `signature_to_ir`'s `Signature::Hash` arm.
    SigHash,
    /// `signature_to_ir`'s `Signature::Compound` arm. `right` is emptied when
    /// the second operand is scheduled, so the machine cannot schedule it twice.
    SigCompound {
        right: Option<SigInput<'ast>>,
        bound_map_chain: BoundMapChain<VarSort>,
        left: Option<Sig>,
    },
    /// `recognize_signed_term` — validate `s`, then lower `P` ordinarily.
    SignedTerm {
        core_inner: AnnProc<'ast>,
        input: ProcVisitInputs,
    },
    /// `recognize_signed_join` — validate every clause signature, then lower the
    /// recovered plain join.
    SignedJoin {
        plain_for: AnnProc<'ast>,
        clause_sigs: Vec<Signature<'ast>>,
        idx: usize,
        bound_map_chain: BoundMapChain<VarSort>,
        input: ProcVisitInputs,
    },
    /// `recognize_token_stack` — validate every layer, then lower to the input
    /// process unchanged.
    TokenStack {
        layers: &'ast [Signature<'ast>],
        idx: usize,
        input: ProcVisitInputs,
    },
}

impl NormKont<'_> {
    /// The **total** number of child values this continuation's chain consumes.
    ///
    /// ★ This deliberately duplicates what the `descend_*`/`combine_*` arms do.
    /// It is not a convenience accessor: it is an *independent statement* of each
    /// node's child count, read off the node's own shape, and the whole point is
    /// to cross-check it against the accumulators the arms actually maintain
    /// (see [`NormKont::filled`] and the slot invariant in [`norm_drive`]).
    ///
    /// The `match` is exhaustive with no `_` arm, so a continuation added without
    /// an arity is a compile error.
    pub(crate) fn arity(&self) -> usize {
        match self {
            // one operand
            NormKont::Unary { .. } => 1,
            NormKont::Negation { .. } => 1,
            NormKont::Eval { .. } => 1,
            NormKont::New { .. } => 1,
            NormKont::Bundle { .. } => 1,
            NormKont::NameQuote => 1,
            NormKont::CanonQuote => 1,
            NormKont::SigHash => 1,
            NormKont::SignedTerm { .. } => 1,

            // two operands
            NormKont::Binary { .. } => 2,
            NormKont::Conjunction { .. } => 2,
            NormKont::Disjunction { .. } => 2,
            NormKont::Matches { .. } => 2,
            NormKont::SigCompound { .. } => 2,

            // `condition`, `if_true`, `if_false`-or-`Nil`
            NormKont::If { .. } => 3,

            // one per flattened `|` operand
            NormKont::ParSeq { procs, .. } => procs.len(),

            // one per element / two per pair
            NormKont::Collect { elements, .. } => elements.len(),
            NormKont::CollectMap { pairs, .. } => 2 * pairs.len(),

            // the channel, then one per message
            NormKont::Send(k) => 1 + k.inputs.len(),
            // the receiver, then one per argument
            NormKont::Method(k) => 1 + k.args.len(),
            // the contract name, then one per formal, then the body
            NormKont::Contr(k) => 1 + k.formals.names.len() + 1,

            // the scrutinee, then per case: pattern (+ guard, if present) + body
            NormKont::Match(k) => {
                1 + k
                    .cases
                    .iter()
                    .map(|c| 2 + usize::from(c.guard.is_some()))
                    .sum::<usize>()
            }

            // every formal of every bind, then every channel, then the optional
            // guard, then the body
            NormKont::Input(k) => {
                k.patterns.iter().map(|(names, _)| names.len()).sum::<usize>()
                    + k.sources.len()
                    + usize::from(k.guard.is_some())
                    + 1
            }

            // one signature per clause / per layer; the lowering itself is a
            // TAIL and consumes no value
            NormKont::SignedJoin { clause_sigs, .. } => clause_sigs.len(),
            NormKont::TokenStack { layers, .. } => layers.len(),
        }
    }

    /// How many child values this continuation has already absorbed — read off
    /// its **accumulators**, never off its arity.
    ///
    /// The exhaustive `match` is the other half of the cross-check: `arity`
    /// speaks for the node's shape, `filled` speaks for what the machine has
    /// actually delivered, and [`norm_drive`] asserts they meet exactly once.
    pub(crate) fn filled(&self) -> usize {
        match self {
            NormKont::Unary { .. }
            | NormKont::Negation { .. }
            | NormKont::Eval { .. }
            | NormKont::New { .. }
            | NormKont::Bundle { .. }
            | NormKont::NameQuote
            | NormKont::CanonQuote
            | NormKont::SigHash
            | NormKont::SignedTerm { .. } => 0,

            NormKont::Binary { left_par, .. } => usize::from(left_par.is_some()),
            NormKont::Conjunction { left_par, .. } => usize::from(left_par.is_some()),
            NormKont::Disjunction { left_par, .. } => usize::from(left_par.is_some()),
            NormKont::Matches { left, .. } => usize::from(left.is_some()),
            NormKont::SigCompound { left, .. } => usize::from(left.is_some()),

            NormKont::If { phase, .. } => match phase {
                IfPhase::Condition => 0,
                IfPhase::TrueCase => 1,
                IfPhase::FalseCase => 2,
            },

            NormKont::ParSeq { idx, .. } => *idx,
            NormKont::Collect { acc_pars, .. } => acc_pars.len(),
            NormKont::CollectMap {
                acc_pairs, key_par, ..
            } => 2 * acc_pairs.len() + usize::from(key_par.is_some()),

            NormKont::Send(k) => k.idx,
            NormKont::Method(k) => k.idx,
            NormKont::Contr(k) => k.idx,

            NormKont::Match(k) => {
                let done: usize = k.cases[..k.case_idx]
                    .iter()
                    .map(|c| 2 + usize::from(c.guard.is_some()))
                    .sum();
                let within = match k.phase {
                    MatchPhase::Target => 0,
                    MatchPhase::Pattern => 0,
                    MatchPhase::Guard => 1,
                    MatchPhase::Body => 1 + usize::from(k.guard_out.is_some()),
                };
                usize::from(k.target.is_some()) + done + within
            }

            NormKont::Input(k) => {
                let all_names: usize = k.patterns.iter().map(|(n, _)| n.len()).sum();
                match k.phase {
                    InputPhase::Patterns {
                        group_idx,
                        name_idx,
                    } => {
                        k.patterns[..group_idx]
                            .iter()
                            .map(|(n, _)| n.len())
                            .sum::<usize>()
                            + name_idx
                    }
                    InputPhase::Sources { idx } => all_names + idx,
                    InputPhase::Guard => all_names + k.sources.len(),
                    InputPhase::Body => {
                        all_names + k.sources.len() + usize::from(k.guard.is_some())
                    }
                }
            }

            NormKont::SignedJoin { idx, .. } => *idx,
            NormKont::TokenStack { idx, .. } => *idx,
        }
    }

    /// Every `Par` this continuation owns, for iterative teardown on the error
    /// path. `Drop` for `Par` is Θ(depth); see the module docs.
    fn into_pars(self) -> Vec<Par> {
        fn push_opt(out: &mut Vec<Par>, p: Option<Par>) {
            out.extend(p);
        }
        let mut out = Vec::new();
        match self {
            NormKont::Unary { input_par, .. } => out.push(input_par),
            NormKont::Binary {
                input_par, left_par, ..
            } => {
                out.push(input_par);
                push_opt(&mut out, left_par);
            }
            NormKont::ParSeq { .. } => {}
            NormKont::Collect {
                acc_pars, input_par, ..
            } => {
                out.extend(acc_pars);
                out.push(input_par);
            }
            NormKont::CollectMap {
                acc_pairs,
                key_par,
                input_par,
                ..
            } => {
                for (k, v) in acc_pairs {
                    out.push(k);
                    out.push(v);
                }
                push_opt(&mut out, key_par);
                out.push(input_par);
            }
            NormKont::Conjunction {
                input, left_par, ..
            }
            | NormKont::Disjunction {
                input, left_par, ..
            } => {
                out.push(input.par);
                push_opt(&mut out, left_par);
            }
            NormKont::Matches { input, left, .. } => {
                out.push(input.par);
                if let Some(l) = left {
                    out.push(l.par);
                }
            }
            NormKont::Negation { input, .. } | NormKont::Bundle { input, .. } => {
                out.push(input.par)
            }
            NormKont::Eval { input_par } => out.push(input_par),
            NormKont::Send(k) => {
                let SendK {
                    name_par,
                    acc_pars,
                    input_par,
                    ..
                } = *k;
                push_opt(&mut out, name_par);
                out.extend(acc_pars);
                out.push(input_par);
            }
            NormKont::Method(k) => {
                let MethodK {
                    target,
                    acc_args,
                    input_par,
                    ..
                } = *k;
                push_opt(&mut out, target);
                out.extend(acc_args);
                out.push(input_par);
            }
            NormKont::If {
                outer_par,
                condition,
                true_case,
                ..
            } => {
                out.push(outer_par);
                out.extend(condition.map(|c| c.par));
                out.extend(true_case.map(|t| t.par));
            }
            NormKont::Match(k) => {
                let MatchK {
                    input,
                    target,
                    acc_cases,
                    pattern_par,
                    guard_out,
                    ..
                } = *k;
                out.push(input.par);
                out.extend(target.map(|t| t.par));
                for c in acc_cases {
                    out.extend(c.pattern);
                    out.extend(c.source);
                    out.extend(c.guard);
                }
                push_opt(&mut out, pattern_par);
                out.extend(guard_out.map(|g| g.par));
            }
            NormKont::New { input_par, .. } => out.push(input_par),
            NormKont::Contr(k) => {
                let ContrK {
                    name_out,
                    acc_patterns,
                    input,
                    ..
                } = *k;
                out.extend(name_out.map(|n| n.par));
                out.extend(acc_patterns);
                out.push(input.par);
            }
            NormKont::Input(k) => {
                let InputK {
                    input,
                    group_pars,
                    done_patterns,
                    source_pars,
                    receive_binds,
                    guard_out,
                    ..
                } = *k;
                out.push(input.par);
                out.extend(group_pars);
                for (pars, _, _, _) in done_patterns {
                    out.extend(pars);
                }
                out.extend(source_pars);
                for b in receive_binds {
                    out.extend(b.patterns);
                    out.extend(b.source);
                }
                out.extend(guard_out.map(|g| g.par));
            }
            NormKont::NameQuote | NormKont::CanonQuote | NormKont::SigHash => {}
            NormKont::SigCompound { .. } => {}
            NormKont::SignedTerm { input, .. }
            | NormKont::SignedJoin { input, .. }
            | NormKont::TokenStack { input, .. } => out.push(input.par),
        }
        out
    }
}

// ===========================================================================
// the step protocol
// ===========================================================================

/// What a `descend_*` or `combine_*` handler asks the driver to do next.
pub(crate) enum Step<'ast> {
    /// The node is complete — publish its value.
    Done(NormVal),
    /// Push `kont`, then descend into `work`. LIFO pops the child first.
    Descend {
        kont: NormKont<'ast>,
        work: NormWork<'ast>,
    },
    /// **Replace** the current obligation. Used by the desugarings, whose
    /// recursive form ends in an unconditional
    /// `normalize_ann_proc(&rewritten, input, …)` — a proper tail call, which
    /// consumes no continuation, so a chain of them is flat.
    Tail(NormWork<'ast>),
}

// ===========================================================================
// the driver
// ===========================================================================

/// The single LIFO loop. Native stack is `O(1)` in source nesting *and* in
/// sibling width; the recursion lives in `work`.
///
/// See the module documentation for the conservation law, the slot invariant,
/// and why the error path dismantles rather than drops.
pub(crate) fn norm_drive<'ast>(
    root: NormWork<'ast>,
    env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<NormVal, InterpreterError> {
    norm_drive_from(Step::Tail(root), env, parser)
}

/// [`norm_drive`], seeded from a [`Step`] rather than from a bare obligation.
///
/// This is what the **bounded entry points** use — `normalize_name`,
/// `normalize_p_let`, `normalize_p_input`, `signature_to_ir`, `canon_quote` —
/// so that a caller outside the SCC can normalize a sub-node without the SCC
/// growing a second implementation. There is exactly one machine; these
/// functions differ from `normalize_ann_proc` only in where they enter it, and
/// each is `O(1)` in native stack for the same reason.
pub(crate) fn norm_drive_from<'ast>(
    initial: Step<'ast>,
    env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<NormVal, InterpreterError> {
    // Sized for a comfortably deep term without a reallocation on the hot path;
    // the machine grows past this without ceremony.
    let mut work: Vec<NormWork<'ast>> = Vec::with_capacity(64);
    let mut vals: Vec<NormVal> = Vec::with_capacity(4);

    // Maintained incrementally so each check is O(1) per step. A rescan of
    // `work` would be O(n) per step and therefore O(n²) over the drive, and the
    // depth-independence gate runs this machine at parameter 4,096.
    let mut descends: usize = 0;

    match initial {
        Step::Done(v) => vals.push(v),
        Step::Descend { kont, work: child } => {
            work.push(NormWork::Combine(kont));
            work.push(child);
            descends = 1;
        }
        Step::Tail(w) => {
            work.push(w);
            descends = 1;
        }
    }

    loop {
        debug_assert_eq!(
            vals.len() + descends,
            1,
            "norm_drive: CONSERVATION LAW VIOLATED (|vals|={}, descends={}). Exactly one \
             obligation — a pending descend or a produced value — may be outstanding at a time; \
             a handler either published a value it was not asked for or dropped one it was.",
            vals.len(),
            descends
        );

        let Some(item) = work.pop() else { break };

        let step = match item {
            NormWork::Combine(kont) => {
                // ⚠ `filled`/`arity` are computed ONLY under `debug_assertions`.
                // Both are `match`es that walk a continuation's shape — `Match`
                // sums over its completed cases, `Input` over its pattern groups
                // — so evaluating them unconditionally would put an `O(n)` read
                // on every `O(1)` machine step and make the drive quadratic in a
                // *release* node. They are diagnostics, not machinery.
                #[cfg(debug_assertions)]
                let (expected, total) = {
                    let filled = kont.filled();
                    let total = kont.arity();
                    debug_assert!(
                        filled < total,
                        "norm_drive: SLOT INVARIANT VIOLATED — a continuation with filled={} and \
                         arity={} is still on the work stack. `NormKont::arity` disagrees with the \
                         accumulators its arms maintain; that is the bug class which silently \
                         renumbers free variables.",
                        filled,
                        total
                    );
                    (filled + 1, total)
                };
                let value = match vals.pop() {
                    Some(v) => v,
                    None => unreachable!("norm_drive: a Combine ran with an empty value stack"),
                };
                let step = match run_combine(kont, value, env, parser) {
                    Ok(s) => s,
                    Err(e) => return Err(unwind(e, work, vals)),
                };
                // A continuation that has just absorbed its LAST child must
                // produce a value; one that has not must schedule exactly one
                // more child. Both directions are asserted, because only the
                // pair pins `arity` against the arms.
                #[cfg(debug_assertions)]
                debug_assert_eq!(
                    matches!(step, Step::Done(_) | Step::Tail(_)),
                    expected == total,
                    "norm_drive: a continuation absorbed child {} of {} and then {}",
                    expected,
                    total,
                    if expected == total {
                        "asked for another"
                    } else {
                        "finished early"
                    }
                );
                step
            }
            descend => {
                descends -= 1;
                match run_descend(descend, env, parser) {
                    Ok(s) => s,
                    Err(e) => return Err(unwind(e, work, vals)),
                }
            }
        };

        match step {
            Step::Done(v) => vals.push(v),
            Step::Descend { kont, work: child } => {
                debug_assert!(
                    !matches!(child, NormWork::Combine(_)),
                    "norm_drive: a Step::Descend must schedule a child, not a continuation"
                );
                work.push(NormWork::Combine(kont));
                work.push(child);
                descends += 1;
            }
            Step::Tail(next) => {
                debug_assert!(
                    !matches!(next, NormWork::Combine(_)),
                    "norm_drive: a Step::Tail must schedule a child, not a continuation"
                );
                work.push(next);
                descends += 1;
            }
        }
    }

    debug_assert_eq!(
        vals.len(),
        1,
        "norm_drive: exactly one value must remain when the work stack drains"
    );
    Ok(vals
        .pop()
        .expect("norm_drive: exactly one value must remain when the work stack drains"))
}

/// Tear the machine down **iteratively** on the error path and hand the error
/// back unchanged.
///
/// `?` in the recursive form discarded pending work by unwinding; here the
/// pending work is a heap structure holding `Par`s, and `<Par as Drop>` is
/// itself Θ(depth) (audit §5 row 10, 470 B/level debug). Letting `work` and
/// `vals` fall out of scope would therefore re-introduce the class on precisely
/// the path a hostile input takes.
#[inline(never)]
fn unwind(
    err: InterpreterError,
    work: Vec<NormWork<'_>>,
    vals: Vec<NormVal>,
) -> InterpreterError {
    let mut pars: Vec<Par> = Vec::new();
    for item in work {
        pars.extend(item.into_pars());
    }
    for v in vals {
        pars.extend(v.into_pars());
    }
    dismantle_all(pars);
    err
}

// ===========================================================================
// dispatch
// ===========================================================================

/// Descend into one obligation.
#[inline]
fn run_descend<'ast>(
    work: NormWork<'ast>,
    env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<Step<'ast>, InterpreterError> {
    match work {
        NormWork::Proc { proc, input } => super::normalize::descend_proc(proc, input, env, parser),
        NormWork::Name { name, input } => {
            super::normalizer::name_normalize_matcher::descend_name(name, input)
        }
        NormWork::CanonQuote { proc } => {
            super::normalizer::cost_accounting::sig::descend_canon_quote(proc)
        }
        NormWork::Sig {
            sig,
            bound_map_chain,
        } => super::normalizer::cost_accounting::sig::descend_sig(sig, bound_map_chain, env),
        NormWork::Combine(_) => unreachable!("norm_drive: handled by the Combine arm"),
    }
}

/// Run one continuation against the child value it was waiting for.
///
/// ★ Each arm delegates to its own `#[inline(never)]` function, for the same
/// reason `sort_combine.rs` splits its 36 `combine_*` arms: at `-O0` `rustc`
/// does not overlap the stack slots of mutually exclusive `match` arms, so a
/// single fat `run_combine` would size its frame for *every* arm at once. That
/// frame sits on the drive loop's only stack level, so it is a one-off constant
/// rather than a per-level cost — but it is a large one-off constant, and
/// splitting bounds each arm by its own locals by construction.
#[inline]
fn run_combine<'ast>(
    kont: NormKont<'ast>,
    value: NormVal,
    env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<Step<'ast>, InterpreterError> {
    use super::normalizer::collection_normalize_matcher as collect;
    use super::normalizer::cost_accounting::{recognize, sig};
    use super::normalizer::name_normalize_matcher as name;
    use super::normalizer::processes as p;

    match kont {
        NormKont::Unary {
            input_par,
            input_depth,
            ctor,
        } => super::normalize::combine_unary(input_par, input_depth, ctor, value),
        NormKont::Binary {
            right,
            input_par,
            input_depth,
            bound_map_chain,
            ctor,
            left_par,
        } => super::normalize::combine_binary(
            right,
            input_par,
            input_depth,
            bound_map_chain,
            ctor,
            left_par,
            value,
        ),
        NormKont::ParSeq {
            procs,
            idx,
            bound_map_chain,
        } => p::p_par_normalizer::combine_p_par(procs, idx, bound_map_chain, value),
        NormKont::Collect {
            kind,
            elements,
            idx,
            acc_pars,
            locally_free,
            connective_used,
            known_free,
            bound_map_chain,
            input_par,
            input_depth,
        } => collect::combine_collect(
            kind,
            elements,
            idx,
            acc_pars,
            locally_free,
            connective_used,
            known_free,
            bound_map_chain,
            input_par,
            input_depth,
            value,
        ),
        NormKont::CollectMap {
            remainder,
            pairs,
            idx,
            on_value,
            key_par,
            acc_pairs,
            locally_free,
            connective_used,
            known_free,
            bound_map_chain,
            input_par,
            input_depth,
        } => collect::combine_collect_map(
            remainder,
            pairs,
            idx,
            on_value,
            key_par,
            acc_pairs,
            locally_free,
            connective_used,
            known_free,
            bound_map_chain,
            input_par,
            input_depth,
            value,
        ),
        NormKont::Conjunction {
            right,
            input,
            span,
            left_par,
        } => p::p_conjunction_normalizer::combine_p_conjunction(right, input, span, left_par, value),
        NormKont::Disjunction {
            right,
            input,
            span,
            left_par,
        } => p::p_disjunction_normalizer::combine_p_disjunction(right, input, span, left_par, value),
        NormKont::Matches { right, input, left } => {
            p::p_matches_normalizer::combine_p_matches(right, input, left, value)
        }
        NormKont::Negation { input, span } => {
            p::p_negation_normalizer::combine_p_negation(input, span, value)
        }
        NormKont::Eval { input_par } => p::p_eval_normalizer::combine_p_eval(input_par, value),
        NormKont::Send(k) => p::p_send_normalizer::combine_p_send(k, value),
        NormKont::Method(k) => p::p_method_normalizer::combine_p_method(k, value),
        NormKont::If {
            if_true,
            if_false,
            bound_map_chain,
            outer_par,
            phase,
            condition,
            true_case,
        } => p::p_if_normalizer::combine_p_if(
            if_true,
            if_false,
            bound_map_chain,
            outer_par,
            phase,
            condition,
            true_case,
            value,
            parser,
        ),
        NormKont::Match(k) => p::p_match_normalizer::combine_p_match(k, value),
        NormKont::New {
            new_count,
            uris,
            input_par,
        } => p::p_new_normalizer::combine_p_new(new_count, uris, input_par, value, env),
        NormKont::Contr(k) => p::p_contr_normalizer::combine_p_contr(k, value),
        NormKont::Bundle {
            bundle_type,
            span,
            input,
        } => p::p_bundle_normalizer::combine_p_bundle(bundle_type, span, input, value),
        NormKont::Input(k) => p::p_input_normalizer::combine_p_input(k, value),
        NormKont::NameQuote => name::combine_name_quote(value),
        NormKont::CanonQuote => sig::combine_canon_quote(value),
        NormKont::SigHash => sig::combine_sig_hash(value),
        NormKont::SigCompound {
            right,
            bound_map_chain,
            left,
        } => sig::combine_sig_compound(right, bound_map_chain, left, value),
        NormKont::SignedTerm { core_inner, input } => {
            recognize::combine_signed_term(core_inner, input, value)
        }
        NormKont::SignedJoin {
            plain_for,
            clause_sigs,
            idx,
            bound_map_chain,
            input,
        } => recognize::combine_signed_join(plain_for, clause_sigs, idx, bound_map_chain, input, value),
        NormKont::TokenStack { layers, idx, input } => {
            recognize::combine_token_stack(layers, idx, input, value)
        }
    }
}
