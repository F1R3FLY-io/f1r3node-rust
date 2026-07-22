// See See rholang/src/main/scala/coop/rchain/rholang/interpreter/Reduce.scala

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use futures::FutureExt;
use smallvec::SmallVec;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use futures::stream::{FuturesUnordered, StreamExt};
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{
    BindPattern, Bundle, EAnd, EDiv, EEq, EGt, EGte, EList, ELt, ELte, EMatches, EMethod, EMinus,
    EMap, EMinusMinus, EMod, EMult, ENeq, EOr, EPathMap, EPercentPercent, EPlus, EPlusPlus, ESet,
    ETuple, EVar,
    EZipper, Expr, GPrivate, GUnforgeable, If, KeyValuePair, ListParWithRandom, Match, MatchCase,
    New, Par, ParWithRandom, Receive, ReceiveBind, Send, TaggedContinuation, Var,
};
use models::rust::par_map::ParMap;
use models::rust::par_map_type_mapper::ParMapTypeMapper;
use models::rust::par_set::ParSet;
use models::rust::par_set_type_mapper::ParSetTypeMapper;
use models::rust::pathmap_crate_type_mapper::PathMapCrateTypeMapper;
use models::rust::pathmap_integration::segments_to_key;
use models::rust::pathmap_native_query::{
    collect_child_segments, collect_subtrie_values, path_prefix_exists,
};
use models::rust::pathmap_zipper::RholangReadZipper;
use models::rust::rholang::implicits::{concatenate_pars, single_bundle, single_expr};
use models::rust::sorted_par_hash_set::SortedParHashSet;
use models::rust::sorted_par_map::SortedParMap;
use models::rust::string_ops::StringOps;
use models::rust::utils::{
    new_elist_par, new_emap_par, new_gint_expr, new_gint_par, new_gstring_par, union,
};
use prost::Message;
use rspace_plus_plus::rspace::logging::ReductionKind;
use rspace_plus_plus::rspace::merger::merging_logic::MergeType;
use rspace_plus_plus::rspace::util::unpack_option_with_peek;
use tokio::sync::RwLock;

use super::accounting::costs::{
    bigint_comparison_cost, bigint_division_cost, bigint_modulo_cost, bigint_multiplication_cost,
    bigint_negation_cost, bigint_subtraction_cost, bigint_sum_cost, bigrat_comparison_cost,
    bigrat_division_cost, bigrat_multiplication_cost, bigrat_negation_cost,
    bigrat_subtraction_cost, bigrat_sum_cost, boolean_and_cost, boolean_or_cost,
    byte_array_append_cost, comparison_cost, division_cost, equality_check_cost, list_append_cost,
    method_call_cost, modulo_cost, multiplication_cost, new_bindings_cost, op_call_cost,
    receive_eval_cost, send_eval_cost, string_append_cost, subtraction_cost, sum_cost,
    var_eval_cost,
};
use super::accounting::RuntimeBudget;
use super::dispatch::{DispatchType, RhoDispatch, RholangAndScalaDispatcher};
use super::env::Env;
use super::errors::InterpreterError;
use super::matcher::has_locally_free::HasLocallyFree;
use super::metering::MeteredMachine;
use super::metrics_constants::{
    REDUCER_EVAL_MATCH_CALLS_METRIC, REDUCER_EVAL_MATCH_TIME_NS_METRIC,
    REDUCER_EVAL_NEW_CALLS_METRIC, REDUCER_EVAL_NEW_TIME_NS_METRIC,
    REDUCER_EVAL_RECEIVE_CALLS_METRIC, REDUCER_EVAL_RECEIVE_TIME_NS_METRIC,
    REDUCER_EVAL_SEND_CALLS_METRIC, REDUCER_EVAL_SEND_TIME_NS_METRIC, RHOLANG_METRICS_SOURCE,
};
use super::rho_runtime::RhoISpace;
use super::rho_type::{RhoExpression, RhoUnforgeable};
use super::substitute::Substitute;
use super::unwrap_option_safe;
use super::util::GeneratedMessage;
use crate::rust::interpreter::accounting::costs::{
    add_cost, bytes_to_hex_cost, diff_cost, hex_to_bytes_cost, interpolate_cost, keys_method_cost,
    length_method_cost, lookup_cost, match_eval_cost, nth_method_call_cost, remove_cost,
    size_method_cost, slice_cost, take_cost, to_byte_array_cost, to_list_cost, union_cost,
};
use crate::rust::interpreter::matcher::spatial_matcher::SpatialMatcherContext;
use crate::rust::interpreter::rho_type::RhoTuple2;

/// Minimum remaining stack space (in bytes) before growing.
/// When the current stack has less than this amount remaining, a new stack segment is allocated.
// 128 KB is too small: a single recursion frame in the Rholang interpreter
// (eval → produce/consume → dispatch → eval) consumes more than 128 KB between
// stacker checks, so the overflow happens before stacker can grow the stack.
const STACK_RED_ZONE: usize = 1024 * 1024; // 1 MB

/// Size of each new stack segment allocated when the red zone is reached.
const STACK_GROW_SIZE: usize = 2 * 1024 * 1024; // 2 MB

/// A Future wrapper that dynamically grows the thread stack during polling.
///
/// The Rholang interpreter uses deep async recursion: eval → produce/consume → dispatch → eval.
/// Each poll of this recursive future chain adds stack frames. In debug builds, unoptimized
/// async state machines consume ~1-2KB per recursion level, causing stack overflow with the
/// default 2MB thread stack.
///
/// `StackGrowingFuture` wraps each recursive entry point (eval, produce, consume, dispatch).
/// On each poll, `stacker::maybe_grow` checks remaining stack space. If below STACK_RED_ZONE,
/// it allocates a new STACK_GROW_SIZE segment and runs the poll there. This allows arbitrarily
/// deep Rholang recursion (e.g., longslow.rho with 32768 iterations) without stack overflow.
///
/// See: https://github.com/F1R3FLY-io/f1r3node/issues/305
/// See: https://github.com/F1R3FLY-io/f1r3node/issues/306
struct StackGrowingFuture<F> {
    inner: F,
}

impl<F: Future> Future for StackGrowingFuture<F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // SAFETY: Structural pin projection on a single-field struct with no Drop impl.
        // `inner` is only accessed through this pinned projection, and StackGrowingFuture
        // does not implement Unpin when F doesn't, preserving pin guarantees.
        let inner = unsafe { self.map_unchecked_mut(|s| &mut s.inner) };
        stacker::maybe_grow(STACK_RED_ZONE, STACK_GROW_SIZE, || inner.poll(cx))
    }
}

// ============================================================================
// Detached-spawn + atomic-counter REDUCTION DRIVER.
//
// tokio::spawn already gives an O(1) async stack, but each parent `await`ing its children pins an
// O(N) parked-parent chain (shortslow: 32768 parked parents). The send is coupled to its
// continuation purely for completion/error plumbing. Detaching the children and tracking completion
// with a global atomic counter restores async-send (deposit + terminate; the continuation runs
// independently) and collapses the chain, while PRESERVING concurrency (the same concurrent spawns)
// => COMM order unchanged => no consensus divergence (proven empirically by the async-driver
// differential harness). `inj` seeds a fresh DriveState per deploy (live init 1 = the root eval),
// and drains it (awaits live -> 0) before aggregating. Each of the five async join sites snapshots
// `self.drive` and `spawn_detached`s its children instead of awaiting them. See the durable spec
// scratchpad/async_counter_driver_plan.
// ============================================================================

/// Per-deploy async completion driver. Shared (behind `self.drive`) across every metering-child
/// clone and reached by the dispatch re-entry, so `inj`'s store is visible to every task.
/// `pub(crate)` only so the two `DebruijnInterpreter` construction sites (rho_runtime.rs +
/// the reduce.rs test constructor) can install the placeholder cell via `idle_drive_cell`.
pub(crate) struct DriveState {
    /// Outstanding tasks (root + all live detached children). Initialised to 1 (the root eval task).
    live: AtomicUsize,
    /// Flat, `Located`-wrapped errors pushed by detached tasks. Push order is non-deterministic;
    /// `inj` sorts by coordinate for a deterministic DISPLAY order (NOT consensus).
    sink: Mutex<Vec<InterpreterError>>,
    /// Fired exactly once, on the 1->0 transition, to wake `inj`.
    done: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}

impl DriveState {
    /// A never-loaded placeholder installed at interpreter construction; `inj` replaces it with a
    /// live driver before any spawn site runs.
    fn idle() -> Self {
        DriveState {
            live: AtomicUsize::new(0),
            sink: Mutex::new(Vec::new()),
            done: Mutex::new(None),
        }
    }
}

/// Build the placeholder drive cell installed at interpreter construction. `pub(crate)` so the two
/// `DebruijnInterpreter` construction sites can set the `drive` field without naming `DriveState`.
pub(crate) fn idle_drive_cell() -> Arc<std::sync::RwLock<Arc<DriveState>>> {
    Arc::new(std::sync::RwLock::new(Arc::new(DriveState::idle())))
}

/// RAII decrement of `live`. Dropped LAST inside each detached task (after the error push), so the
/// sink is complete when `done` fires. Covers Ok / Err / `?` / panic — no missed decrement.
struct LiveGuard(Arc<DriveState>);

impl Drop for LiveGuard {
    fn drop(&mut self) {
        if self.0.live.fetch_sub(1, Ordering::AcqRel) == 1 {
            if let Some(tx) = self
                .0
                .done
                .lock()
                .expect("DriveState.done mutex poisoned")
                .take()
            {
                let _ = tx.send(());
            }
        }
    }
}

/// Spawn `fut` as a detached task counted by `drive`. INCREMENT-BEFORE-SPAWN + the RAII guard
/// guarantee `live` never reaches 0 prematurely and never misses a decrement. `catch_unwind` is
/// MANDATORY: a panicking deploy must still record an error, else `is_failed` would flip to success.
/// The child's `Ok(_)` value is discarded — the five sites only ever conveyed Skip/error upward
/// (NonDeterministicCall / FailedNonDeterministicCall arise only from the direct-await `else`
/// branches, never one of the five sites) — only `Err` and panics are captured, wrapped in `Located`.
fn spawn_detached<T: std::marker::Send + 'static>(
    drive: &Arc<DriveState>,
    child_path: SmallVec<[u32; 8]>,
    fut: Pin<Box<dyn Future<Output = Result<T, InterpreterError>> + std::marker::Send>>,
) {
    drive.live.fetch_add(1, Ordering::AcqRel); // INCREMENT BEFORE SPAWN
    let drive = drive.clone();
    tokio::spawn(async move {
        // `_guard` drops LAST (after the push below), so the decrement happens AFTER the error lands
        // in the sink — the sink is therefore complete when `done` fires.
        let _guard = LiveGuard(drive.clone());
        let outcome = AssertUnwindSafe(fut).catch_unwind().await;
        match outcome {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => drive
                .sink
                .lock()
                .expect("DriveState.sink mutex poisoned")
                .push(InterpreterError::Located {
                    path: child_path.into_vec(),
                    source: Box::new(e),
                }),
            Err(_panic) => drive
                .sink
                .lock()
                .expect("DriveState.sink mutex poisoned")
                .push(InterpreterError::Located {
                    path: child_path.into_vec(),
                    source: Box::new(InterpreterError::ReduceError(
                        "reduction task panicked".to_string(),
                    )),
                }),
        }
    });
}

/// The coordinate (path) carried by a `Located` error, or `&[]` for a bare error. Used by `inj` to
/// sort the sink into a deterministic DISPLAY order (NOT consensus).
fn located_path(e: &InterpreterError) -> &[u32] {
    match e {
        InterpreterError::Located { path, .. } => path,
        _ => &[],
    }
}

/**
 * Reduce is the interface for evaluating Rholang expressions.
 */
#[derive(Clone)]
pub struct DebruijnInterpreter {
    pub space: RhoISpace,
    pub dispatcher: RhoDispatch,
    pub urn_map: Arc<HashMap<String, Par>>,
    pub merge_chs: Arc<RwLock<HashMap<Par, MergeType>>>,
    pub mergeable_tags: Arc<HashMap<Par, MergeType>>,
    pub metering: MeteredMachine,
    pub substitute: Substitute,
    /// Async completion driver cell. Shared across every `#[derive(Clone)]` metering-child clone
    /// (the outer `Arc`) and replaceable per deploy (`RwLock`): `inj` stores a fresh `DriveState`
    /// before evaluating and the five join sites snapshot it via `load_drive`. Internal machinery
    /// (not `pub`); holds an `idle` placeholder until `inj` seeds it. `std::sync::RwLock` is spelled
    /// out to avoid clashing with the `tokio::sync::RwLock` import above.
    pub(crate) drive: Arc<std::sync::RwLock<Arc<DriveState>>>,
}

type Application = Option<(
    TaggedContinuation,
    Vec<(Par, ListParWithRandom, ListParWithRandom, bool)>,
    bool,
)>;

trait Method {
    fn apply(&self, p: Par, args: Vec<Par>, env: &Env<Par>) -> Result<Par, InterpreterError>;
}

// ============================================================================
// LEG-2: explicit-worklist TRAMPOLINE for the expression evaluator SCC.
//
// The six mutually-recursive expression evaluators (`eval_expr`,
// `eval_expr_to_par`, `eval_expr_to_expr`, `eval_single_expr`, `eval_to_bool`,
// `eval_to_i64`) form a post-order fold whose call-stack depth is Theta(term
// nesting depth). Deeply-nested ground terms (e.g. `((..(1+1)..)+1)` or
// `[[..[0]..]]`) overflow the 8 MiB stack at depth ~1.5k / ~0.75k. Leg-2
// replaces the CALL stack with an explicit HEAP worklist (`drive`), keeping the
// native stack O(1) while preserving byte-identical results and — crucially for
// a consensus reducer — a byte-identical charge (cost) trace.
//
// Structure:
//   * `EvVal`  — a produced value, tagged by the producing evaluator's return
//                type (Par / Expr / bool / i64).
//   * `EvWork` — a pending unit of work: evaluate a borrowed AST node under one
//                of the six evaluators, or run a post-order `Combine`.
//   * `EvKont` — the post-order continuation for a `Combine`: it names the arm
//                being reassembled and carries the borrowed AST metadata (and
//                child counts) the arm needs that are NOT child values.
//   * `drive`  — the single LIFO loop: pop work; a `descend_*` handler either
//                pushes a leaf value, or (pre-order charge, then) pushes
//                `Combine(k)` followed by the children in REVERSE so they pop
//                left-to-right; a `Combine` pops the children's values, runs the
//                arm's post-order body (post-order charge + build) and pushes the
//                result value. Exactly one value remains at the end.
//
// Charge preservation (the crux — see the module test `differential`):
//   * PRE-order charges (`method_call_cost`; `op_call_cost` for %% / ++ / --)
//     run in the `descend_*` handler BEFORE any child is pushed.
//   * POST-order charges run in `Combine` AFTER all child values are available.
//   * Each arm's INTERNAL statement order (e.g. reserve(division_cost) THEN the
//     rhs==0 check) is reproduced verbatim by the shared `combine_*` helper.
//   * A `?` abort discards `work`/`vals` and leaves already-reserved charges
//     reserved — identical to the recursive `?`.
//
// Owned intermediates (eval_var results, method-apply results, %% map contents,
// Set/Map arithmetic results, and the SORTED elements of Set/Map literals) are
// NOT the deep structural spine; they are re-evaluated by DIRECT calls to the
// (now trampolined) wrappers, each starting a fresh bounded `drive`. This keeps
// the native stack O(1) for the reported plus/list overflow AND for method
// chains (whose deep target is a worklisted child), while matching the recursive
// evaluator's behaviour on the shallow re-eval paths.
// ============================================================================

/// A produced value, tagged by the evaluator that produced it.
enum EvVal {
    Par(Par),
    Expr(Expr),
    Bool(bool),
    I64(i64),
}

/// A pending unit of work. All AST references borrow the input term (`'e`); the
/// worklist therefore performs ZERO deep clones on the structural spine.
enum EvWork<'e> {
    /// `eval_expr(par)`  -> EvVal::Par
    EEval(&'e Par),
    /// `eval_expr_to_par(expr)`  -> EvVal::Par
    EToPar(&'e Expr),
    /// `eval_expr_to_expr(expr)`  -> EvVal::Expr
    EToExpr(&'e Expr),
    /// `eval_single_expr(par)`  -> EvVal::Expr
    ESingle(&'e Par),
    /// `eval_to_bool(par)`  -> EvVal::Bool
    EBool(&'e Par),
    /// `eval_to_i64(par)`  -> EvVal::I64
    EI64(&'e Par),
    /// Post-order reassembly.
    Combine(EvKont<'e>),
}

/// Post-order continuation. Names the arm + carries borrowed metadata / child
/// counts (never child values — those live on the `vals` stack).
enum EvKont<'e> {
    // ---- eval_expr ----
    /// Fold `concatenate_pars` over `n` child Pars onto the shell of `par`.
    Join { par: &'e Par, n: usize },
    // ---- eval_expr_to_par ----
    /// `Par::default().with_exprs(vec![child_expr])`.
    ToParWrap,
    /// `method.apply(target, args)` -> Par (Display "Unimplemented method" error).
    ToParMethod { emethod: &'e EMethod, argc: usize },
    // ---- eval_expr_to_expr (unary) ----
    Neg,
    Not,
    // ---- eval_expr_to_expr (binary numeric / relational) ----
    Mult,
    Div,
    Mod,
    Plus,
    Minus,
    Relop {
        relopb: fn(bool, bool) -> bool,
        relopi: fn(i64, i64) -> bool,
        relops: fn(String, String) -> bool,
    },
    // ---- eval_expr_to_expr (equality / boolean / matches) ----
    Eq,
    Neq,
    And,
    Or,
    Matches { pattern: &'e Par },
    // ---- eval_expr_to_expr (interpolation / append / remove) ----
    PercentPercent,
    PlusPlus,
    MinusMinus,
    // ---- eval_expr_to_expr (collections that worklist their elements) ----
    EListK { e1: &'e EList, n: usize },
    ETupleK { e1: &'e ETuple, n: usize },
    EPathmapK { e1: &'e EPathMap, n: usize },
    // ---- eval_expr_to_expr (method) ----
    EMethodExprK { emethod: &'e EMethod, argc: usize },
    // ---- eval_to_bool / eval_to_i64 ([e] arm: extract from an evaluated Expr) ----
    BoolExtract,
    I64Extract,
}

// ---------------------------------------------------------------------------
// value-stack pop helpers (type discipline: the producing EvWork guarantees the
// variant; a mismatch is a trampoline bug, hence `expect`).
// ---------------------------------------------------------------------------
#[inline]
fn ev_pop_par(vals: &mut Vec<EvVal>) -> Par {
    match vals.pop() {
        Some(EvVal::Par(p)) => p,
        _ => unreachable!("eval_drive: expected Par on value stack"),
    }
}
#[inline]
fn ev_pop_expr(vals: &mut Vec<EvVal>) -> Expr {
    match vals.pop() {
        Some(EvVal::Expr(e)) => e,
        _ => unreachable!("eval_drive: expected Expr on value stack"),
    }
}
#[inline]
fn ev_pop_bool(vals: &mut Vec<EvVal>) -> bool {
    match vals.pop() {
        Some(EvVal::Bool(b)) => b,
        _ => unreachable!("eval_drive: expected Bool on value stack"),
    }
}
// (no `ev_pop_i64`: an i64 value is only ever produced as a drive ROOT — for the
// `eval_to_i64` wrapper / `nth` — and never pushed as a child, so no `Combine`
// pops one off the value stack.)
/// Pop `n` Pars, returning them in FORWARD (push) order.
#[inline]
fn ev_pop_n_par(vals: &mut Vec<EvVal>, n: usize) -> Vec<Par> {
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(ev_pop_par(vals));
    }
    out.reverse();
    out
}

/// By-reference equivalent of `expr.locally_free(expr.clone(), 0)` (the
/// `<Expr as HasLocallyFree<Expr>>::locally_free` reader, whose `&self` is unused
/// and whose `e` param is only read for CACHED `locally_free` fields — never
/// recursed into children). The by-value form deep-CLONES the whole subtree per
/// element, an O(depth) recursive clone that reintroduces the stack overflow on
/// nested collections AFTER the eval SCC is trampolined; this reads the cached
/// bitset in O(1) per node. Byte-identical result (validated by the differential
/// harness). `EVar` clones only the SHALLOW `Var`, matching the original.
fn expr_locally_free_ref(expr: &Expr) -> Vec<u8> {
    match &expr.expr_instance {
        Some(ExprInstance::GBool(_)) => Vec::new(),
        Some(ExprInstance::GInt(_)) => Vec::new(),
        Some(ExprInstance::GDouble(_)) => Vec::new(),
        Some(ExprInstance::GBigInt(_)) => Vec::new(),
        Some(ExprInstance::GBigRat(_)) => Vec::new(),
        Some(ExprInstance::GFixedPoint(_)) => Vec::new(),
        Some(ExprInstance::GString(_)) => Vec::new(),
        Some(ExprInstance::GUri(_)) => Vec::new(),
        Some(ExprInstance::GByteArray(_)) => Vec::new(),

        Some(ExprInstance::EListBody(e)) => e.locally_free.clone(),
        Some(ExprInstance::ETupleBody(e)) => e.locally_free.clone(),
        Some(ExprInstance::ESetBody(e)) => e.locally_free.clone(),
        Some(ExprInstance::EMapBody(e)) => e.locally_free.clone(),
        Some(ExprInstance::EPathmapBody(e)) => e.locally_free.clone(),
        Some(ExprInstance::EZipperBody(e)) => e.locally_free.clone(),

        Some(ExprInstance::EVarBody(EVar { v })) => v.clone().unwrap().locally_free(v.clone().unwrap(), 0),
        Some(ExprInstance::ENotBody(enot)) => enot.p.as_ref().unwrap().locally_free.clone(),
        Some(ExprInstance::ENegBody(eneg)) => eneg.p.as_ref().unwrap().locally_free.clone(),

        Some(ExprInstance::EMultBody(EMult { p1, p2 })) => {
            union(p1.as_ref().unwrap().locally_free.clone(), p2.as_ref().unwrap().locally_free.clone())
        }
        Some(ExprInstance::EDivBody(EDiv { p1, p2 })) => {
            union(p1.as_ref().unwrap().locally_free.clone(), p2.as_ref().unwrap().locally_free.clone())
        }
        Some(ExprInstance::EModBody(EMod { p1, p2 })) => {
            union(p1.as_ref().unwrap().locally_free.clone(), p2.as_ref().unwrap().locally_free.clone())
        }
        Some(ExprInstance::EPlusBody(EPlus { p1, p2 })) => {
            union(p1.as_ref().unwrap().locally_free.clone(), p2.as_ref().unwrap().locally_free.clone())
        }
        Some(ExprInstance::EMinusBody(EMinus { p1, p2 })) => {
            union(p1.as_ref().unwrap().locally_free.clone(), p2.as_ref().unwrap().locally_free.clone())
        }
        Some(ExprInstance::ELtBody(ELt { p1, p2 })) => {
            union(p1.as_ref().unwrap().locally_free.clone(), p2.as_ref().unwrap().locally_free.clone())
        }
        Some(ExprInstance::ELteBody(ELte { p1, p2 })) => {
            union(p1.as_ref().unwrap().locally_free.clone(), p2.as_ref().unwrap().locally_free.clone())
        }
        Some(ExprInstance::EGtBody(EGt { p1, p2 })) => {
            union(p1.as_ref().unwrap().locally_free.clone(), p2.as_ref().unwrap().locally_free.clone())
        }
        Some(ExprInstance::EGteBody(EGte { p1, p2 })) => {
            union(p1.as_ref().unwrap().locally_free.clone(), p2.as_ref().unwrap().locally_free.clone())
        }
        Some(ExprInstance::EEqBody(EEq { p1, p2 })) => {
            union(p1.as_ref().unwrap().locally_free.clone(), p2.as_ref().unwrap().locally_free.clone())
        }
        Some(ExprInstance::ENeqBody(ENeq { p1, p2 })) => {
            union(p1.as_ref().unwrap().locally_free.clone(), p2.as_ref().unwrap().locally_free.clone())
        }
        Some(ExprInstance::EAndBody(EAnd { p1, p2 })) => {
            union(p1.as_ref().unwrap().locally_free.clone(), p2.as_ref().unwrap().locally_free.clone())
        }
        Some(ExprInstance::EOrBody(EOr { p1, p2 })) => {
            union(p1.as_ref().unwrap().locally_free.clone(), p2.as_ref().unwrap().locally_free.clone())
        }

        Some(ExprInstance::EMethodBody(e)) => e.locally_free.clone(),
        Some(ExprInstance::EMatchesBody(EMatches { target, .. })) => target.as_ref().unwrap().locally_free.clone(),

        Some(ExprInstance::EPercentPercentBody(EPercentPercent { p1, p2 })) => {
            union(p1.as_ref().unwrap().locally_free.clone(), p2.as_ref().unwrap().locally_free.clone())
        }
        Some(ExprInstance::EPlusPlusBody(EPlusPlus { p1, p2 })) => {
            union(p1.as_ref().unwrap().locally_free.clone(), p2.as_ref().unwrap().locally_free.clone())
        }
        Some(ExprInstance::EMinusMinusBody(EMinusMinus { p1, p2 })) => {
            union(p1.as_ref().unwrap().locally_free.clone(), p2.as_ref().unwrap().locally_free.clone())
        }

        None => Vec::new(),
    }
}


/**
 * Materialize a send in the store, optionally returning the matched continuation.
 *
 * @param chan  The channel on which data is being sent.
 * @param data  The par objects holding the processes being sent.
 * @param persistent  True if the write should remain in the tuplespace indefinitely.
 */
impl DebruijnInterpreter {
    fn with_metering_child(&self, component: usize) -> Self {
        let metering = self.metering.child(component.min(u32::MAX as usize) as u32);
        let mut child = self.clone();
        child.metering = metering.clone();
        child.substitute = Substitute { metering };
        child
    }

    /// Public reduction entry point (STABLE signature — external callers, tests). Seeds the empty
    /// parallel-tree coordinate; coordinate threading is internal machinery (see `eval_with_path`).
    pub fn eval<'a>(
        &'a self,
        par: Par,
        env: &'a Env<Par>,
        rand: Blake2b512Random,
    ) -> Pin<
        Box<
            dyn std::future::Future<Output = Result<(), InterpreterError>> + std::marker::Send + 'a,
        >,
    > {
        self.eval_with_path(par, env, rand, SmallVec::new())
    }

    /// Coordinate-threaded reduction entry (async counter driver). `path` is this eval task's position
    /// in the parallel-reduction tree; the width-N terms in `eval_inner` fan out at `path + [i]`.
    pub(crate) fn eval_with_path<'a>(
        &'a self,
        par: Par,
        env: &'a Env<Par>,
        rand: Blake2b512Random,
        path: SmallVec<[u32; 8]>,
    ) -> Pin<
        Box<
            dyn std::future::Future<Output = Result<(), InterpreterError>> + std::marker::Send + 'a,
        >,
    > {
        Box::pin(StackGrowingFuture {
            inner: self.eval_inner(par, env, rand, path),
        })
    }

    async fn eval_inner(
        &self,
        par: Par,
        env: &Env<Par>,
        rand: Blake2b512Random,
        // Coordinate of THIS eval task in the parallel-reduction tree. Each of the width-N parallel
        // terms below is evaluated at `path + [i]` (the term index i also seeds the rand split), so a
        // detached branch's error is localizable. Display-only (NOT consensus).
        path: SmallVec<[u32; 8]>,
    ) -> Result<(), InterpreterError> {
        let terms: Vec<GeneratedMessage> = vec![
            par.sends
                .into_iter()
                .map(GeneratedMessage::Send)
                .collect::<Vec<_>>(),
            par.receives
                .into_iter()
                .map(GeneratedMessage::Receive)
                .collect(),
            par.news.into_iter().map(GeneratedMessage::New).collect(),
            par.matches
                .into_iter()
                .map(GeneratedMessage::Match)
                .collect(),
            par.conditionals
                .into_iter()
                .map(GeneratedMessage::If)
                .collect(),
            par.bundles
                .into_iter()
                .map(GeneratedMessage::Bundle)
                .collect(),
            par.exprs
                .into_iter()
                .filter(|expr| match &expr.expr_instance {
                    Some(expr_instance) => match expr_instance {
                        ExprInstance::EVarBody(_) => true,
                        ExprInstance::EMethodBody(_) => true,
                        _ => false,
                    },
                    None => false,
                })
                .collect::<Vec<Expr>>()
                .into_iter()
                .map(GeneratedMessage::Expr)
                .collect(),
        ]
        .into_iter()
        .filter(|vec| !vec.is_empty())
        .flatten()
        .collect();
        // Split-id routing by parallel width. Term indices are 0-based, so a
        // width-N Par produces ids 0..=N-1:
        //   - width <= 128: every id (0..=127) is `i8`-representable —
        //     `split_byte` (ONE domain-separation path byte). Byte-identical
        //     to the historical behavior; consensus-relevant, do not change.
        //   - width in [129, 256]: ids reach 128..=255, which overflow `i8`.
        //     The old boundary (`> 256`) still sent these widths to
        //     `split_byte`, so `id.try_into().unwrap()` panicked with
        //     `PosOverflow` at id 128 — this range previously produced NO
        //     output (it always crashed). It now joins the `split_short`
        //     path (TWO little-endian path bytes) used by every larger width.
        //   - width > 256: ids fit `i16` (the Par is capped at
        //     `term_split_limit = i16::MAX` terms below) — `split_short`,
        //     unchanged.
        // The boundary sits at 128 because that is the largest width whose
        // maximum id (127) still fits `i8`. A `split_short` child appends two
        // path bytes where a `split_byte` child appends one, so the rerouted
        // range cannot collide with any defined `split_byte` output of the
        // same parent generator.
        fn split(
            id: i32,
            terms: &Vec<GeneratedMessage>,
            rand: Blake2b512Random,
        ) -> Blake2b512Random {
            if terms.len() == 1 {
                rand
            } else if terms.len() > 128 {
                rand.split_short(
                    id.try_into()
                        .expect("term index must fit i16: widths are capped at i16::MAX terms"),
                )
            } else {
                rand.split_byte(
                    id.try_into()
                        .expect("term index must fit i8 for parallel widths <= 128"),
                )
            }
        }

        let term_split_limit = i16::MAX;
        if terms.len() > term_split_limit.try_into().unwrap() {
            Err(InterpreterError::ReduceError(format!(
                "The number of terms in the Par is {}, which exceeds the limit of {}",
                terms.len(),
                term_split_limit
            )))
        } else {
            // Collect errors from all parallel execution paths (pars)
            // parTraverseSafe
            let futures: Vec<
                Pin<
                    Box<
                        dyn futures::Future<Output = Result<(), InterpreterError>>
                            + std::marker::Send
                            + 'static,
                    >,
                >,
            > = terms
                .iter()
                .enumerate()
                .map(|(index, term)| {
                    let self_clone = self.with_metering_child(index);
                    let term_clone = term.clone();
                    let env_clone = env.clone();
                    let rand_split = split(index.try_into().unwrap(), &terms, rand.clone());
                    // Child coordinate for parallel term `index` (matches the rand split index).
                    let mut child = path.clone();
                    child.push(index as u32);
                    Box::pin(async move {
                        self_clone
                            .generated_message_eval(&term_clone, &env_clone, rand_split, child)
                            .await
                    })
                        as Pin<
                            Box<
                                dyn futures::Future<Output = Result<(), InterpreterError>>
                                    + std::marker::Send
                                    + 'static,
                            >,
                        >
                })
                .collect();

            metrics::counter!("reducer.eval_par.calls", "source" => "rholang").increment(1);
            metrics::counter!("reducer.eval_par.term_count", "source" => "rholang")
                .increment(futures.len() as u64);

            // SITE 1 (eval_inner par-join) — DETACHED. Instead of `tokio::spawn` + `await` (which pins
            // an O(N) parked-parent chain), detach each parallel branch as a COUNTED task on the deploy's
            // drive: errors flow to the drive sink (Located at the branch coordinate) and surface at
            // `inj` drain. Concurrency is UNCHANGED (still one task per branch) => COMM order unchanged
            // => no consensus divergence (proven by the differential harness). The parent returns
            // immediately with Ok(()).
            let drive = self.drive.read().expect("drive cell poisoned").clone();
            for (index, fut) in futures.into_iter().enumerate() {
                let mut child = path.clone();
                child.push(index as u32);
                spawn_detached(&drive, child, fut);
            }
            Ok(())
        }
    }

    pub async fn inj(&self, par: Par, rand: Blake2b512Random) -> Result<(), InterpreterError> {
        // Seed a FRESH completion driver for THIS deploy (live=1 = the root eval task) and install it so
        // every spawn site AND the dispatch re-entry observe it via `self.drive`. Deploys are serialized
        // by `evaluate(&mut self)`, and `inj` fully drains (awaits live -> 0) before returning, so a
        // single live driver at a time.
        let (tx, rx) = tokio::sync::oneshot::channel();
        let drive = Arc::new(DriveState {
            live: AtomicUsize::new(1),
            sink: Mutex::new(Vec::new()),
            done: Mutex::new(Some(tx)),
        });
        *self.drive.write().expect("drive cell poisoned") = drive.clone();

        // The root eval runs as task 0 at coordinate []. Its OWN (synchronous) error is recorded
        // directly here; detached children record theirs through `spawn_detached`.
        let root = LiveGuard(drive.clone());
        let env = Env::new();
        if let Err(e) = self.eval(par, &env, rand).await {
            drive
                .sink
                .lock()
                .expect("DriveState.sink mutex poisoned")
                .push(InterpreterError::Located {
                    path: Vec::new(),
                    source: Box::new(e),
                });
        }
        drop(root); // may fire `done` immediately if every detached child already finished
        let _ = rx.await; // wake once live -> 0 (root + every detached task complete)

        // Deterministic DISPLAY order (by coordinate). NOT consensus — `aggregate_evaluator_errors`
        // classifies on `root_cause()` (Located-unwrapping), preserving the exact cost discriminant.
        let mut errors =
            std::mem::take(&mut *drive.sink.lock().expect("DriveState.sink mutex poisoned"));
        errors.sort_by(|a, b| located_path(a).cmp(located_path(b)));
        match self.aggregate_evaluator_errors(errors) {
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        }
    }

    /**
     * Materialize a send in the store, optionally returning the matched continuation.
     *
     * @param chan  The channel on which data is being sent.
     * @param data  The par objects holding the processes being sent.
     * @param persistent  True if the write should remain in the tuplespace indefinitely.
     */
    fn produce<'a>(
        &'a self,
        chan: Par,
        data: ListParWithRandom,
        persistent: bool,
        path: SmallVec<[u32; 8]>,
    ) -> Pin<
        Box<
            dyn std::future::Future<Output = Result<DispatchType, InterpreterError>>
                + std::marker::Send
                + 'a,
        >,
    > {
        Box::pin(StackGrowingFuture {
            inner: self.produce_inner(chan, data, persistent, path),
        })
    }

    /// Reactive single-step per-reduction seam (MeTTaIL OSLF stepper): emit `redex` to the installed
    /// observer, then — if a live step session is active — pause on the shared [`StepGate`] until the
    /// stepper releases the next step. `None` in production: two branch-predicted `is_none` checks, no
    /// emit, no await. The redex is borrowed (the observer clones only when it captures). This is the
    /// non-COMM twin of the COMM gate pause in `produce_inner`/`consume_inner`.
    async fn observe_and_pause(
        &self,
        redex: &Par,
        kind: ReductionKind,
    ) -> Result<(), InterpreterError> {
        self.space.observe_reduction(redex, kind);
        if let Some(gate) = self.space.step_gate() {
            gate.pause().await.map_err(|_| {
                InterpreterError::ReduceError("single-step session aborted".to_string())
            })?;
        }
        Ok(())
    }

    async fn produce_inner(
        &self,
        chan: Par,
        data: ListParWithRandom,
        persistent: bool,
        path: SmallVec<[u32; 8]>,
    ) -> Result<DispatchType, InterpreterError> {
        self.update_mergeable_channels(&chan).await;
        // EPathMap fix P4.1 (plan §0.C "produce_inner clones chan+data",
        // reduce.rs:399 clone removal): the space CONSUMES the channel and
        // payload — the only post-call consumer is the persistent-produce
        // re-fire in `continue_produce_process`, so only a persistent send
        // retains a copy. A non-persistent send (the common case) hands its
        // payload to the space with zero copies.
        let refire = if persistent {
            Some((chan.clone(), data.clone()))
        } else {
            None
        };
        let produce_result = self.space.produce(chan, data, persistent).await?;
        let is_replay = self.space.is_replay().await;

        match produce_result {
            Some((c, s, produce_event)) => {
                // Reactive single-step back-pressure (MeTTaIL OSLF stepper). The COMM has committed
                // and its event was already emitted by the observer inside `space.produce`; the
                // per-channel tuplespace lock was dropped when `produce` returned, so we hold no
                // lock here. If a live step session is active, pause (cooperative async yield,
                // parks the task not the thread) until the stepper releases the next step. `None`
                // in production — one observer `is_none` check, no await.
                if let Some(gate) = self.space.step_gate() {
                    gate.pause().await.map_err(|_| {
                        InterpreterError::ReduceError("single-step session aborted".to_string())
                    })?;
                }
                let dispatch_type = self
                    .continue_produce_process(
                        unpack_option_with_peek(Some((c, s))),
                        refire,
                        persistent,
                        is_replay,
                        produce_event.clone().output_value,
                        produce_event.failed,
                        path,
                    )
                    .await?;

                match dispatch_type {
                    DispatchType::NonDeterministicCall(ref output) => {
                        let produce1 = produce_event.mark_as_non_deterministic(output.clone());
                        self.space.update_produce(produce1).await;
                        Ok(dispatch_type)
                    }

                    DispatchType::FailedNonDeterministicCall(error) => {
                        // Mark the produce as failed for replay safety
                        let failed_produce = produce_event.with_error();
                        self.space.update_produce(failed_produce).await;
                        // Re-raise known error types as-is to preserve output_not_produced;
                        // wrap unknown errors in NonDeterministicProcessFailure.
                        match error {
                            InterpreterError::ProduceFailureWithOutput { .. }
                            | InterpreterError::NonDeterministicProcessFailure { .. } => Err(error),
                            _ => Err(InterpreterError::NonDeterministicProcessFailure {
                                cause: Box::new(error),
                                output_not_produced: vec![],
                            }),
                        }
                    }

                    _ => Ok(dispatch_type),
                }
            }
            // A `produce` with no matching consumer deposits the send and returns Skip. This is NOT
            // a per-reduction step: it fires for an internal send awaiting a future receive (whose
            // rendezvous IS the COMM step) just as much as for a truly-resting output, and the order
            // is non-deterministic — so emitting here would spuriously show consumed sends. A resting
            // output is the residual of the reduction, surfaced as the final reduction's result (the
            // dereferenced/continuation body), not as its own reduction.
            None => Ok(DispatchType::Skip),
        }
    }

    fn consume<'a>(
        &'a self,
        binds: Vec<(BindPattern, Par)>,
        body: ParWithRandom,
        persistent: bool,
        peek: bool,
        guard: Option<Par>,
        path: SmallVec<[u32; 8]>,
    ) -> Pin<
        Box<
            dyn std::future::Future<Output = Result<DispatchType, InterpreterError>>
                + std::marker::Send
                + 'a,
        >,
    > {
        Box::pin(StackGrowingFuture {
            inner: self.consume_inner(binds, body, persistent, peek, guard, path),
        })
    }

    async fn consume_inner(
        &self,
        binds: Vec<(BindPattern, Par)>,
        body: ParWithRandom,
        persistent: bool,
        peek: bool,
        guard: Option<Par>,
        path: SmallVec<[u32; 8]>,
    ) -> Result<DispatchType, InterpreterError> {
        let (patterns, sources): (Vec<BindPattern>, Vec<Par>) = binds.clone().into_iter().unzip();

        // Update mergeable channels
        for source in &sources {
            self.update_mergeable_channels(source).await;
        }

        let consume_result = self
            .space
            .consume(
                sources.clone(),
                patterns.clone(),
                TaggedContinuation {
                    tagged_cont: Some(TaggedCont::ParBody(body.clone())),
                    guard: guard.clone(),
                },
                persistent,
                if peek {
                    BTreeSet::from_iter((0..sources.len() as i32).collect::<Vec<i32>>())
                } else {
                    BTreeSet::new()
                },
            )
            .await?;
        let is_replay = self.space.is_replay().await;

        // Reactive single-step back-pressure (MeTTaIL OSLF stepper): pause only when this consume
        // actually fired a COMM (matched waiting produces) — its event was already emitted inside
        // `space.consume`, and no tuplespace lock is held here. `None` in production.
        if consume_result.is_some() {
            if let Some(gate) = self.space.step_gate() {
                gate.pause().await.map_err(|_| {
                    InterpreterError::ReduceError("single-step session aborted".to_string())
                })?;
            }
        }

        self.continue_consume_process(
            unpack_option_with_peek(consume_result),
            binds,
            body,
            persistent,
            peek,
            is_replay,
            Vec::new(),
            guard,
            path,
        )
        .await
    }

    async fn continue_produce_process(
        &self,
        res: Application,
        // P4.1: `Some((chan, data))` iff the produce was persistent — the
        // ONLY arm that needs the original send again (the re-fire below);
        // every other arm runs payload-copy-free.
        refire: Option<(Par, ListParWithRandom)>,
        persistent: bool,
        is_replay: bool,
        previous_output: Vec<Vec<u8>>,
        trace_failed: bool,
        path: SmallVec<[u32; 8]>,
    ) -> Result<DispatchType, InterpreterError> {
        // During replay, if the trace shows a failed non-deterministic process,
        // we cannot replay it - the external service call failed during original execution
        if is_replay && trace_failed {
            return Err(InterpreterError::CanNotReplayFailedNonDeterministicProcess);
        }

        let previous_output_as_par = previous_output
            .into_iter()
            .map(|bytes| {
                Par::decode(&bytes[..]).map_err(|e| InterpreterError::DecodeError(e.to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?;

        match res {
            Some((continuation, data_list, peek)) => {
                if persistent {
                    // dispatchAndRun
                    let self_clone1 = self.with_metering_child(0);
                    let self_clone2 = self.with_metering_child(1);
                    let continuation_clone = continuation.clone();
                    let data_list_clone = data_list.clone();
                    let previous_output_clone = previous_output_as_par.clone();
                    // P4.1: the retained copy moves into the re-fire future
                    // (no further clone — pre-P4.1 this was the SECOND copy).
                    let (chan_refire, data_refire) = refire.expect(
                        "persistent produce retains its channel+payload for the re-fire \
                         (produce_inner built refire = Some for persistent sends)",
                    );
                    let persistent_flag = persistent;
                    let is_replay_flag = is_replay;
                    // Coordinate continuity: the dispatch + re-fire continuations carry the produce's
                    // path (run_parallel_dispatches adds the fan-out index at the detached spawn).
                    let path_dispatch = path.clone();
                    let path_refire = path.clone();

                    let mut futures: Vec<
                        Pin<
                            Box<
                                dyn futures::Future<Output = Result<DispatchType, InterpreterError>>
                                    + std::marker::Send
                                    + 'static,
                            >,
                        >,
                    > = vec![];

                    futures.push(Box::pin(async move {
                        self_clone1
                            .dispatch(
                                continuation_clone,
                                data_list_clone,
                                is_replay_flag,
                                previous_output_clone,
                                path_dispatch,
                            )
                            .await
                    })
                        as Pin<
                            Box<
                                dyn futures::Future<Output = Result<DispatchType, InterpreterError>>
                                    + std::marker::Send
                                    + 'static,
                            >,
                        >);

                    futures.push(Box::pin(async move {
                        self_clone2
                            .produce(chan_refire, data_refire, persistent_flag, path_refire)
                            .await
                    })
                        as Pin<
                            Box<
                                dyn futures::Future<Output = Result<DispatchType, InterpreterError>>
                                    + std::marker::Send
                                    + 'static,
                            >,
                        >);

                    // When a persistent produce triggers a peek COMM, the non-persistent
                    // peeked data on other channels was removed by RSpace. Re-issue it
                    // to preserve peek semantics (data should remain after peek read).
                    if peek {
                        futures.extend(self.produce_peeks(data_list, 2, path.clone()).await);
                    }

                    self.run_parallel_dispatches(futures, path).await
                } else if peek {
                    // dispatchAndRun
                    let self_clone = self.with_metering_child(0);
                    let continuation_clone = continuation.clone();
                    let data_list_clone = data_list.clone();
                    let previous_output_clone = previous_output_as_par.clone();
                    let path_dispatch = path.clone();

                    let mut futures: Vec<
                        Pin<
                            Box<
                                dyn futures::Future<Output = Result<DispatchType, InterpreterError>>
                                    + std::marker::Send
                                    + 'static,
                            >,
                        >,
                    > = vec![Box::pin(async move {
                        self_clone
                            .dispatch(
                                continuation_clone,
                                data_list_clone,
                                is_replay,
                                previous_output_clone,
                                path_dispatch,
                            )
                            .await
                    })];
                    futures.extend(self.produce_peeks(data_list, 1, path.clone()).await);

                    self.run_parallel_dispatches(futures, path).await
                } else {
                    self.dispatch(continuation, data_list, is_replay, previous_output_as_par, path)
                        .await
                }
            }
            None => Ok(DispatchType::Skip),
        }
    }

    async fn continue_consume_process(
        &self,
        res: Application,
        binds: Vec<(BindPattern, Par)>,
        body: ParWithRandom,
        persistent: bool,
        peek: bool,
        is_replay: bool,
        previous_output: Vec<Vec<u8>>,
        guard: Option<Par>,
        path: SmallVec<[u32; 8]>,
    ) -> Result<DispatchType, InterpreterError> {
        let previous_output_as_par = previous_output
            .into_iter()
            .map(|bytes| {
                Par::decode(&bytes[..]).map_err(|e| InterpreterError::DecodeError(e.to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?;

        match res {
            Some((continuation, data_list, _peek)) => {
                if persistent {
                    // dispatchAndRun
                    let self_clone1 = self.with_metering_child(0);
                    let self_clone2 = self.with_metering_child(1);
                    let continuation_clone = continuation.clone();
                    let data_list_clone = data_list.clone();
                    let previous_output_clone = previous_output_as_par.clone();
                    let binds_clone = binds.clone();
                    let body_clone = body.clone();
                    let persistent_flag = persistent;
                    let peek_flag = peek;
                    let is_replay_flag = is_replay;
                    let guard_clone = guard.clone();
                    // Coordinate continuity: dispatch + re-install continuations carry the consume's path.
                    let path_dispatch = path.clone();
                    let path_reconsume = path.clone();

                    let mut futures: Vec<
                        Pin<
                            Box<
                                dyn futures::Future<Output = Result<DispatchType, InterpreterError>>
                                    + std::marker::Send
                                    + 'static,
                            >,
                        >,
                    > = vec![];

                    futures.push(Box::pin(async move {
                        self_clone1
                            .dispatch(
                                continuation_clone,
                                data_list_clone,
                                is_replay_flag,
                                previous_output_clone,
                                path_dispatch,
                            )
                            .await
                    })
                        as Pin<
                            Box<
                                dyn futures::Future<Output = Result<DispatchType, InterpreterError>>
                                    + std::marker::Send
                                    + 'static,
                            >,
                        >);

                    futures.push(Box::pin(async move {
                        self_clone2
                            .consume(
                                binds_clone,
                                body_clone,
                                persistent_flag,
                                peek_flag,
                                guard_clone,
                                path_reconsume,
                            )
                            .await
                    })
                        as Pin<
                            Box<
                                dyn futures::Future<Output = Result<DispatchType, InterpreterError>>
                                    + std::marker::Send
                                    + 'static,
                            >,
                        >);

                    self.run_parallel_dispatches(futures, path).await
                } else if _peek {
                    // dispatchAndRun
                    let self_clone = self.with_metering_child(0);
                    let continuation_clone = continuation.clone();
                    let data_list_clone = data_list.clone();
                    let previous_output_clone = previous_output_as_par.clone();
                    let path_dispatch = path.clone();

                    let mut futures: Vec<
                        Pin<
                            Box<
                                dyn futures::Future<Output = Result<DispatchType, InterpreterError>>
                                    + std::marker::Send
                                    + 'static,
                            >,
                        >,
                    > = vec![Box::pin(async move {
                        self_clone
                            .dispatch(
                                continuation_clone,
                                data_list_clone,
                                is_replay,
                                previous_output_clone,
                                path_dispatch,
                            )
                            .await
                    })];
                    futures.extend(self.produce_peeks(data_list, 1, path.clone()).await);

                    self.run_parallel_dispatches(futures, path).await
                } else {
                    self.dispatch(continuation, data_list, is_replay, previous_output_as_par, path)
                        .await
                }
            }
            None => Ok(DispatchType::Skip),
        }
    }

    fn dispatch<'a>(
        &'a self,
        continuation: TaggedContinuation,
        data_list: Vec<(Par, ListParWithRandom, ListParWithRandom, bool)>,
        is_replay: bool,
        previous_output: Vec<Par>,
        path: SmallVec<[u32; 8]>,
    ) -> Pin<
        Box<
            dyn std::future::Future<Output = Result<DispatchType, InterpreterError>>
                + std::marker::Send
                + 'a,
        >,
    > {
        Box::pin(StackGrowingFuture {
            inner: self.dispatch_inner(continuation, data_list, is_replay, previous_output, path),
        })
    }

    async fn dispatch_inner(
        &self,
        continuation: TaggedContinuation,
        data_list: Vec<(Par, ListParWithRandom, ListParWithRandom, bool)>,
        is_replay: bool,
        previous_output: Vec<Par>,
        path: SmallVec<[u32; 8]>,
    ) -> Result<DispatchType, InterpreterError> {
        self.dispatcher
            .dispatch(
                continuation,
                data_list.into_iter().map(|tuple| tuple.1).collect(),
                is_replay,
                previous_output,
                path,
            )
            .await
    }

    async fn produce_peeks(
        &self,
        data_list: Vec<(Par, ListParWithRandom, ListParWithRandom, bool)>,
        start_component: usize,
        path: SmallVec<[u32; 8]>,
    ) -> Vec<
        Pin<
            Box<
                dyn futures::Future<Output = Result<DispatchType, InterpreterError>>
                    + std::marker::Send
                    + 'static,
            >,
        >,
    > {
        data_list
            .into_iter()
            .filter(|(_, _, _, persist)| !persist)
            .enumerate()
            .map(|(index, (chan, _, removed_data, _))| {
                let self_clone = self.with_metering_child(start_component + index);
                let path_peek = path.clone();
                Box::pin(async move {
                    self_clone.produce(chan, removed_data, false, path_peek).await
                })
                    as Pin<
                        Box<
                            dyn futures::Future<Output = Result<DispatchType, InterpreterError>>
                                + std::marker::Send
                                + 'static,
                        >,
                    >
            })
            .collect()
    }

    async fn run_parallel_dispatches(
        &self,
        futures: Vec<
            Pin<
                Box<
                    dyn futures::Future<Output = Result<DispatchType, InterpreterError>>
                        + std::marker::Send
                        + 'static,
                >,
            >,
        >,
        path: SmallVec<[u32; 8]>,
    ) -> Result<DispatchType, InterpreterError> {
        // `path` is the coordinate base for the detached continuation spawns (used when this join is
        // detached in a following commit); the fan-out index is appended per continuation.
        let _ = &path;
        let mut unordered = FuturesUnordered::new();
        for (index, fut) in futures.into_iter().enumerate() {
            // Persistent/peek continuations must progress independently; spawning
            // preserves parallel execution and isolates deep recursive branches.
            let handle = tokio::spawn(fut);
            unordered.push(async move { (index, handle.await) });
        }

        let mut errors = Vec::new();
        while let Some((index, joined)) = unordered.next().await {
            match joined {
                Ok(Ok(_)) => {}
                Ok(Err(err)) => errors.push((index, err)),
                Err(join_error) => errors.push((
                    index,
                    InterpreterError::ReduceError(format!(
                        "parallel dispatch task failed: {join_error}"
                    )),
                )),
            }
        }

        errors.sort_by_key(|(index, _)| *index);
        let stable_errors = errors.into_iter().map(|(_, err)| err).collect();
        self.aggregate_evaluator_errors(stable_errors)
    }

    /* Collect mergeable channels */

    async fn update_mergeable_channels(&self, chan: &Par) -> () {
        if let Some(merge_type) = self.is_mergeable_channel(chan) {
            let mut merge_chs_write = self.merge_chs.write().await;
            merge_chs_write.insert(chan.clone(), merge_type);
        }
    }

    fn is_mergeable_channel(&self, chan: &Par) -> Option<MergeType> {
        // Hot path — runs on every channel produce/consume. Borrow the head
        // Par of the first ETupleBody expression without allocating.
        metrics::counter!("is-mergeable-channel.calls", "source" => "f1r3fly.rholang.reduce")
            .increment(1);

        let head: Option<&Par> = chan.exprs.iter().find_map(|y| match &y.expr_instance {
            Some(ExprInstance::ETupleBody(etuple)) => etuple.ps.first(),
            _ => None,
        });

        let result = head.and_then(|h| self.mergeable_tags.get(h).copied());

        // Diagnostic trace: every channel write/consume invokes this. Logs
        // distinguish (a) tuple channels that match a registered tag (mergeable),
        // (b) tuple channels with a head that ISN'T in the tag registry
        // (potential bitmask-tag-binding miss), and (c) non-tuple channels
        // (most channels). Configure with `RUST_LOG=f1r3fly.merge.tag_check=trace`.
        if let Some(head_par) = head {
            match result {
                Some(mt) => tracing::trace!(
                    target: "f1r3fly.merge.tag_check",
                    "mergeable channel detected: merge_type={:?}",
                    mt,
                ),
                None => {
                    use prost::Message;
                    let head_bytes = head_par.encode_to_vec();
                    let head_hex: String =
                        head_bytes.iter().map(|b| format!("{:02x}", b)).collect();
                    let tag_hexes: Vec<String> = self
                        .mergeable_tags
                        .keys()
                        .map(|k| {
                            let bs = k.encode_to_vec();
                            bs.iter().map(|b| format!("{:02x}", b)).collect()
                        })
                        .collect();
                    tracing::trace!(
                        target: "f1r3fly.merge.tag_check",
                        "tuple channel with non-tag head: head_hex={}, registered_tag_hexes={:?}",
                        head_hex,
                        tag_hexes,
                    );
                }
            }
        }

        result
    }

    fn aggregate_evaluator_errors(
        &self,
        errors: Vec<InterpreterError>,
    ) -> Result<DispatchType, InterpreterError> {
        match errors.as_slice() {
            // No errors
            [] => Ok(DispatchType::Skip),

            // Out Of Phlogiston or User Abort error is always single
            // - if one execution path hits these, the whole evaluation stops as well
            // UserAbortError takes precedence over OutOfPhlogistonsError
            // Use single-pass find() to avoid double iteration.
            // COST-DISCRIMINANT SAFETY (async counter driver): detached tasks wrap their errors in
            // `Located`, so classify on `root_cause()` — a Located-wrapped UserAbort/OOP must NOT fall
            // through to the generic single/aggregate arms (wrong cost -> ReplayCostMismatch).
            err_list
                if err_list
                    .iter()
                    .find(|e| matches!(e.root_cause(), InterpreterError::UserAbortError))
                    .is_some() =>
            {
                Err(InterpreterError::UserAbortError)
            }

            err_list
                if err_list
                    .iter()
                    .find(|e| matches!(e.root_cause(), InterpreterError::OutOfPhlogistonsError))
                    .is_some() =>
            {
                Err(InterpreterError::OutOfPhlogistonsError)
            }

            // Rethrow single error — unwrap any `Located` so handle_error sees the raw variant it
            // classifies on (e.g. IfConditionTypeError / OperatorNotDefined cost arms).
            [ex] => Err(ex.root_cause().clone()),

            // Collect errors from parallel execution. KEEP the `Located` elements here: handle_error's
            // Aggregate arm ignores element variants and this preserves the per-lane coordinate for
            // DISPLAY (the elements' Display renders "[path] source").
            err_list => Err(InterpreterError::AggregateError {
                interpreter_errors: err_list.to_vec(),
            }),
        }
    }

    async fn generated_message_eval(
        &self,
        term: &GeneratedMessage,
        env: &Env<Par>,
        rand: Blake2b512Random,
        path: SmallVec<[u32; 8]>,
    ) -> Result<(), InterpreterError> {
        match term {
            GeneratedMessage::Send(term) => {
                metrics::counter!(REDUCER_EVAL_SEND_CALLS_METRIC, "source" => RHOLANG_METRICS_SOURCE)
                    .increment(1);
                let start = std::time::Instant::now();
                let result = self.eval_send(term, env, rand, path).await;
                metrics::counter!(REDUCER_EVAL_SEND_TIME_NS_METRIC, "source" => RHOLANG_METRICS_SOURCE)
                    .increment(start.elapsed().as_nanos() as u64);
                result
            }
            GeneratedMessage::Receive(term) => {
                metrics::counter!(REDUCER_EVAL_RECEIVE_CALLS_METRIC, "source" => RHOLANG_METRICS_SOURCE)
                    .increment(1);
                let start = std::time::Instant::now();
                let result = self.eval_receive(term, env, rand, path).await;
                metrics::counter!(REDUCER_EVAL_RECEIVE_TIME_NS_METRIC, "source" => RHOLANG_METRICS_SOURCE)
                    .increment(start.elapsed().as_nanos() as u64);
                result
            }
            GeneratedMessage::New(term) => {
                metrics::counter!(REDUCER_EVAL_NEW_CALLS_METRIC, "source" => RHOLANG_METRICS_SOURCE)
                    .increment(1);
                let start = std::time::Instant::now();
                let result = self.eval_new(term, env.clone(), rand, path).await;
                metrics::counter!(REDUCER_EVAL_NEW_TIME_NS_METRIC, "source" => RHOLANG_METRICS_SOURCE)
                    .increment(start.elapsed().as_nanos() as u64);
                result
            }
            GeneratedMessage::Match(term) => {
                metrics::counter!(REDUCER_EVAL_MATCH_CALLS_METRIC, "source" => RHOLANG_METRICS_SOURCE)
                    .increment(1);
                let start = std::time::Instant::now();
                let result = self.eval_match(term, env, rand, path).await;
                metrics::counter!(REDUCER_EVAL_MATCH_TIME_NS_METRIC, "source" => RHOLANG_METRICS_SOURCE)
                    .increment(start.elapsed().as_nanos() as u64);
                result
            }
            GeneratedMessage::If(term) => self.eval_if(term, env, rand, path).await,
            GeneratedMessage::Bundle(term) => self.eval_bundle(term, env, rand, path).await,
            GeneratedMessage::Expr(term) => match &term.expr_instance {
                Some(expr_instance) => match expr_instance {
                    ExprInstance::EVarBody(e) => {
                        let res = self.eval_var(&e.clone().v.unwrap(), env)?;
                        // Reactive per-reduction seam: dereference `*N` — `res` is the resolved quoted
                        // process about to be evaluated. Two `is_none` checks, no alloc in production.
                        self.observe_and_pause(&res, ReductionKind::Deref).await?;
                        self.eval_with_path(res, env, rand, path).await
                    }
                    ExprInstance::EMethodBody(e) => {
                        let res = self.eval_expr_to_par(
                            &Expr {
                                expr_instance: Some(ExprInstance::EMethodBody(e.clone())),
                            },
                            env,
                        )?;
                        // Reactive per-reduction seam: method call re-eval. None-op in production.
                        self.observe_and_pause(&res, ReductionKind::Method).await?;
                        self.eval_with_path(res, env, rand, path).await
                    }
                    other => Err(InterpreterError::BugFoundError(format!(
                        "Undefined term: {:?}",
                        other
                    ))),
                },
                None => Err(InterpreterError::BugFoundError(
                    "Undefined term, expr_instance was None".to_string(),
                )),
            },
        }
    }

    /** Algorithm as follows:
     *
     * 1. Fully evaluate the channel in given environment.
     * 2. Substitute any variable references in the channel so that it can be
     *    correctly used as a key in the tuple space.
     * 3. Evaluate any top level expressions in the data being sent.
     * 4. Call produce
     *
     * @param send An output process
     * @param env An execution context
     *
     */
    async fn eval_send(
        &self,
        send: &Send,
        env: &Env<Par>,
        rand: Blake2b512Random,
        path: SmallVec<[u32; 8]>,
    ) -> Result<(), InterpreterError> {
        // D3 (DR-9, OD-3): a send is a token-consuming COMM — the consensus
        // cost unit (one token per COMM). `send_eval_cost` is the diagnostic
        // weight; only the COMM count gates consensus.
        self.metering.reserve_comm(send_eval_cost())?;
        let eval_chan = self.eval_expr(&unwrap_option_safe(send.chan.clone())?, env)?;
        let sub_chan = self.substitute.substitute_and_charge(&eval_chan, 0, env)?;
        let unbundled = match single_bundle(&sub_chan) {
            Some(value) => {
                if !value.write_flag {
                    return Err(InterpreterError::ReduceError(
                        "Trying to send on non-writeable channel.".to_string(),
                    ));
                } else {
                    unwrap_option_safe(value.body)?
                }
            }
            None => sub_chan,
        };

        // W1 Phase 3: per-redex located-stack attribution. The COMM was already
        // charged (scalar, to the envelope) above; this records the per-lane VIEW
        // if the resolved channel is a signer supply channel `Σ⟦sᵢ⟧`. No-op on the
        // single-signer fast path (gated by `any_signed_regions`).
        self.metering.note_channel_lane(&unbundled);

        let subst_data = send
            .data
            .iter()
            .map(|expr| {
                let evaluated = self.eval_expr(expr, env)?;
                self.substitute.substitute_and_charge(&evaluated, 0, env)
            })
            .collect::<Result<Vec<_>, InterpreterError>>()?;

        self.produce(
            unbundled,
            ListParWithRandom {
                pars: subst_data,
                random_state: rand.to_bytes(),
            },
            send.persistent,
            path,
        )
        .await?;
        Ok(())
    }

    async fn eval_receive(
        &self,
        receive: &Receive,
        env: &Env<Par>,
        rand: Blake2b512Random,
        path: SmallVec<[u32; 8]>,
    ) -> Result<(), InterpreterError> {
        // D3 (DR-9, OD-3): a receive is a token-consuming COMM — the consensus
        // cost unit (one token per COMM). `receive_eval_cost` is the diagnostic
        // weight; only the COMM count gates consensus.
        self.metering.reserve_comm(receive_eval_cost())?;

        // Optional `where`-clause guard. Substituted at depth=1 so any
        // variables in scope at the receive site (but not pattern-bound)
        // get replaced with their values, while pattern-bound free vars
        // stay as free vars for the matcher to fill in. Stored once on
        // the TaggedContinuation so it sees every bound variable across
        // every bind. Plan §7.12.
        let subst_guard = match receive.condition.as_ref() {
            Some(c) if c != &Par::default() => {
                Some(self.substitute.substitute_and_charge(c, 1, env)?)
            }
            _ => None,
        };

        let binds = receive
            .binds
            .clone()
            .into_iter()
            .map(|rb| {
                let q = self.unbundle_receive(&rb, env)?;
                let subst_patterns = rb
                    .patterns
                    .into_iter()
                    .map(|pattern| self.substitute.substitute_and_charge(&pattern, 1, env))
                    .collect::<Result<Vec<_>, InterpreterError>>()?;

                Ok((
                    BindPattern {
                        patterns: subst_patterns,
                        remainder: rb.remainder,
                        free_count: rb.free_count,
                    },
                    q,
                ))
            })
            .collect::<Result<Vec<_>, InterpreterError>>()?;

        // W1 Phase 3: per-redex located-stack attribution. A receive is ONE COMM
        // (already charged scalar to the envelope above); record the per-lane VIEW
        // keyed on the FIRST bind's resolved source channel — the SAME bind
        // `delta_sigma::demand_by_sig` attributes on, so the static dual and the
        // runtime agree COMM-for-COMM. No-op on the single-signer fast path.
        if let Some((_, first_channel)) = binds.first() {
            self.metering.note_channel_lane(first_channel);
        }

        // TODO: Allow for the environment to be stored with the body in the Tuplespace - OLD
        let subst_body = self.substitute.substitute_no_sort_and_charge(
            receive.body.as_ref().unwrap(),
            0,
            &env.shift(receive.bind_count),
        )?;

        self.consume(
            binds,
            ParWithRandom {
                body: Some(subst_body),
                random_state: rand.to_bytes(),
            },
            receive.persistent,
            receive.peek,
            subst_guard,
            path,
        )
        .await?;
        Ok(())
    }

    /**
     * Variable "evaluation" is an environment lookup, but
     * lookup of an unbound variable should be an error.
     *
     * @param valproc The variable to be evaluated
     * @param env  provides the environment (possibly) containing a binding for the given variable.
     * @return If the variable has a binding (par), lift the
     *                  binding into the monadic context, else signal
     *                  an exception.
     */
    fn eval_var(&self, valproc: &Var, env: &Env<Par>) -> Result<Par, InterpreterError> {
        self.metering.reserve_primitive(var_eval_cost())?;
        match valproc.var_instance {
            Some(VarInstance::BoundVar(level)) => match env.get(&level) {
                Some(p) => Ok(p),
                None => Err(InterpreterError::ReduceError(format!(
                    "Unbound variable: {} in {:?}",
                    level, env.env_map
                ))),
            },
            Some(VarInstance::Wildcard(_)) => Err(InterpreterError::ReduceError(
                "Unbound variable: attempting to evaluate a pattern".to_string(),
            )),
            Some(VarInstance::FreeVar(_)) => Err(InterpreterError::ReduceError(
                "Unbound variable: attempting to evaluate a pattern".to_string(),
            )),
            None => Err(InterpreterError::ReduceError(
                "Impossible var instance EMPTY".to_string(),
            )),
        }
    }

    // TODO: review 'loop' matches 'tailRecM'
    async fn eval_match(
        &self,
        mat: &Match,
        env: &Env<Par>,
        rand: Blake2b512Random,
        path: SmallVec<[u32; 8]>,
    ) -> Result<(), InterpreterError> {
        fn add_to_env(env: &Env<Par>, free_map: BTreeMap<i32, Par>, free_count: i32) -> Env<Par> {
            (0..free_count).fold(env.clone(), |mut acc, e| {
                let value = free_map.get(&e).unwrap_or(&Par::default()).clone();
                acc.put(value)
            })
        }

        let first_match = Box::new(
            |target: Par, cases: Vec<MatchCase>, rand: Blake2b512Random| async {
                let mut state = (target, cases);

                loop {
                    let (_target, _cases) = state;

                    match _cases.as_slice() {
                        [] => return Ok(()),

                        [single_case, case_rem @ ..] => {
                            let pattern = self.substitute.substitute_and_charge(
                                &unwrap_option_safe(single_case.pattern.clone())?,
                                1,
                                env,
                            )?;

                            let mut spatial_matcher = SpatialMatcherContext::new();
                            let match_result =
                                spatial_matcher.spatial_match_result(_target.clone(), pattern);

                            match match_result {
                                None => {
                                    state = (_target, case_rem.to_vec());
                                }

                                Some(free_map) => {
                                    let case_env =
                                        add_to_env(env, free_map.clone(), single_case.free_count);

                                    // Optional `where` guard. Fire the case
                                    // body iff the guard evaluates to
                                    // GBool(true). Anything else (false,
                                    // non-bool, eval-error) falls through to
                                    // the next case — matching the plan §3.4
                                    // fall-through rule. `Some(empty Par)` is
                                    // treated as "no guard" so we agree with
                                    // eval_receive and Matcher::check_commit.
                                    let guard_passes = match &single_case.guard {
                                        Some(g) if g != &Par::default() => {
                                            match rho_pure_eval::eval(g, &case_env) {
                                                Ok(result) => extract_bool(&result) == Some(true),
                                                Err(_) => false,
                                            }
                                        }
                                        _ => true,
                                    };

                                    if !guard_passes {
                                        state = (_target, case_rem.to_vec());
                                        continue;
                                    }

                                    let case_body = single_case
                                        .source
                                        .clone()
                                        .expect("MatchCase.source: protobuf no_box invariant");
                                    // Reactive per-reduction seam: a `match` case body firing.
                                    // None-op in production.
                                    self.observe_and_pause(&case_body, ReductionKind::Match).await?;
                                    self.eval_with_path(case_body, &case_env, rand, path.clone()).await?;

                                    return Ok(());
                                }
                            }
                        }
                    }
                }
            },
        );

        // D3 (DR-9, OD-3): `match` is a non-COMM structural reduction —
        // DIAGNOSTIC only (it is metered for fidelity but contributes 0 to the
        // consensus consumed cost).
        self.metering.reserve_reduction(match_eval_cost())?;
        let evaled_target = self.eval_expr(
            mat.target
                .as_ref()
                .expect("Match.target: normalizer post-condition"),
            env,
        )?;
        let subst_target = self
            .substitute
            .substitute_and_charge(&evaled_target, 0, env)?;

        first_match(subst_target, mat.cases.clone(), rand).await
    }

    async fn eval_if(
        &self,
        conditional: &If,
        env: &Env<Par>,
        rand: Blake2b512Random,
        path: SmallVec<[u32; 8]>,
    ) -> Result<(), InterpreterError> {
        // D3 (DR-9, OD-3): `if` is a non-COMM structural reduction —
        // DIAGNOSTIC only (metered for fidelity, 0 toward consensus cost).
        self.metering.reserve_reduction(match_eval_cost())?;
        let evaled_cond = self.eval_expr(
            conditional
                .condition
                .as_ref()
                .expect("If.condition: normalizer post-condition"),
            env,
        )?;
        let subst_cond = self
            .substitute
            .substitute_and_charge(&evaled_cond, 0, env)?;

        match extract_bool(&subst_cond) {
            Some(true) => {
                let branch = conditional
                    .if_true
                    .clone()
                    .expect("If.if_true: normalizer post-condition");
                // Reactive per-reduction seam: an `if` true-branch firing. None-op in production.
                self.observe_and_pause(&branch, ReductionKind::If).await?;
                self.eval_with_path(branch, env, rand, path).await
            }
            Some(false) => {
                let branch = conditional
                    .if_false
                    .clone()
                    .expect("If.if_false: normalizer post-condition");
                // Reactive per-reduction seam: an `if` false-branch firing. None-op in production.
                self.observe_and_pause(&branch, ReductionKind::If).await?;
                self.eval_with_path(branch, env, rand, path).await
            }
            None => Err(InterpreterError::IfConditionTypeError {
                actual_type: describe_par_type(&subst_cond),
            }),
        }
    }

    /**
     * Adds neu.bindCount new GPrivate from UUID's to the environment and then
     * proceeds to evaluate the body.
     */
    // TODO: Eliminate variable shadowing - OLD
    async fn eval_new(
        &self,
        new: &New,
        env: Env<Par>,
        mut rand: Blake2b512Random,
        path: SmallVec<[u32; 8]>,
    ) -> Result<(), InterpreterError> {
        let mut alloc = |count: usize, urns: Vec<String>| {
            let simple_news =
                (0..(count - urns.len()))
                    .into_iter()
                    .fold(env.clone(), |mut _env: Env<Par>, _| {
                        let addr: Par = Par::default().with_unforgeables(vec![GUnforgeable {
                            unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
                                id: rand.next().iter().map(|&x| x as u8).collect::<Vec<u8>>(),
                            })),
                        }]);
                        _env.put(addr)
                    });

            let add_urn = |new_env: &mut Env<Par>, urn: String| {
                if !self.urn_map.contains_key(&urn) {
                    // TODO: Injections (from normalizer) are not used currently, see [[NormalizerEnv]].
                    // If `urn` can't be found in `urnMap`, it must be referencing an injection - OLD
                    match new.injections.get(&urn) {
                        Some(p) => {
                            if let Some(gunf) = RhoUnforgeable::unapply(p) {
                                if let Some(instance) = gunf.unf_instance {
                                    Ok(new_env.put(Par::default().with_unforgeables(vec![
                                        GUnforgeable {
                                            unf_instance: Some(instance),
                                        },
                                    ])))
                                } else {
                                    Err(InterpreterError::BugFoundError(
                                        "unf_instance field is None".to_string(),
                                    ))
                                }
                            } else if let Some(expr) = RhoExpression::unapply(p) {
                                if let Some(instance) = expr.expr_instance {
                                    Ok(new_env.put(Par::default().with_exprs(vec![Expr {
                                        expr_instance: Some(instance),
                                    }])))
                                } else {
                                    Err(InterpreterError::BugFoundError(
                                        "expr_instance field is None".to_string(),
                                    ))
                                }
                            } else {
                                Err(InterpreterError::BugFoundError(
                                    "invalid injection".to_string(),
                                ))
                            }
                        }
                        None => Err(InterpreterError::BugFoundError(format!(
                            "No value set for {}. This is a bug in the normalizer or on the path from it.",
                            urn
                        ))),
                    }
                } else {
                    match self.urn_map.get(&urn) {
                        Some(p) => {
                            if urn == "rho:system:bitmaskMergeableTag" {
                                use prost::Message;
                                let bytes = p.encode_to_vec();
                                let hex: String =
                                    bytes.iter().map(|b| format!("{:02x}", b)).collect();
                                tracing::info!(
                                    target: "f1r3fly.merge.tag_check",
                                    "URI lookup at deploy: rho:system:bitmaskMergeableTag -> Par hex={}",
                                    hex,
                                );
                            }
                            Ok(new_env.put(p.clone()))
                        }
                        None => Err(InterpreterError::ReduceError(format!(
                            "Unknown urn for new: {}",
                            urn
                        ))),
                    }
                }
            };

            urns.iter().try_fold(simple_news, |mut acc, urn| {
                add_urn(&mut acc, urn.to_string())
            })
        };

        // D3 (DR-9, OD-3): `new` (name allocation) is a non-COMM structural
        // reduction — DIAGNOSTIC only (metered for fidelity, 0 toward the
        // consensus consumed cost). §7.4 re-pins 9→8 precisely because the
        // `new` no longer counts toward the per-COMM consensus cost.
        self.metering
            .reserve_reduction(new_bindings_cost(new.bind_count as i64))?;
        match alloc(new.bind_count as usize, new.uri.clone()) {
            Ok(env) => {
                let body = unwrap_option_safe(new.p.clone())?;
                // Reactive per-reduction seam: a `new` scope body, after fresh-name allocation.
                // None-op in production.
                self.observe_and_pause(&body, ReductionKind::New).await?;
                self.eval_with_path(body, &env, rand, path).await
            }
            Err(e) => Err(e),
        }
    }

    fn unbundle_receive(&self, rb: &ReceiveBind, env: &Env<Par>) -> Result<Par, InterpreterError> {
        let eval_src = self.eval_expr(&unwrap_option_safe(rb.source.clone())?, env)?;
        let subst = self.substitute.substitute_and_charge(&eval_src, 0, env)?;
        // Check if we try to read from bundled channel
        let unbndl = match single_bundle(&subst) {
            Some(value) => {
                if !value.read_flag {
                    return Err(InterpreterError::ReduceError(
                        "Trying to read from non-readable channel.".to_string(),
                    ));
                } else {
                    value.body.unwrap()
                }
            }
            None => subst,
        };

        Ok(unbndl)
    }

    async fn eval_bundle(
        &self,
        bundle: &Bundle,
        env: &Env<Par>,
        rand: Blake2b512Random,
        path: SmallVec<[u32; 8]>,
    ) -> Result<(), InterpreterError> {
        let body = unwrap_option_safe(bundle.body.clone())?;
        // Reactive per-reduction seam: a `bundle` body, after unwrapping. None-op in production.
        self.observe_and_pause(&body, ReductionKind::Bundle).await?;
        self.eval_with_path(body, env, rand, path).await
    }

    // Public here for testing purposes


    // =======================================================================
    // The single driver loop. Native stack is O(1); recursion lives in `work`.
    // =======================================================================
    fn eval_drive<'e>(
        &self,
        root: EvWork<'e>,
        env: &Env<Par>,
    ) -> Result<EvVal, InterpreterError> {
        let mut work: Vec<EvWork<'e>> = Vec::with_capacity(64);
        let mut vals: Vec<EvVal> = Vec::with_capacity(64);
        work.push(root);
        while let Some(w) = work.pop() {
            match w {
                EvWork::EEval(p) => self.descend_eval(p, env, &mut work, &mut vals)?,
                EvWork::EToPar(e) => self.descend_to_par(e, env, &mut work, &mut vals)?,
                EvWork::EToExpr(e) => self.descend_to_expr(e, env, &mut work, &mut vals)?,
                EvWork::ESingle(p) => self.descend_single(p, env, &mut work, &mut vals)?,
                EvWork::EBool(p) => self.descend_bool(p, env, &mut work, &mut vals)?,
                EvWork::EI64(p) => self.descend_i64(p, env, &mut work, &mut vals)?,
                EvWork::Combine(k) => self.combine(k, env, &mut vals)?,
            }
        }
        Ok(vals.pop().expect("eval_drive: exactly one value must remain"))
    }

    // =======================================================================
    // The six SCC entry points — now THIN wrappers over `eval_drive`. Their
    // signatures are unchanged, so every external caller and every method-body
    // callback is covered transitively (a callback that re-enters simply starts
    // a fresh bounded `drive`).
    // =======================================================================
    pub fn eval_expr(&self, par: &Par, env: &Env<Par>) -> Result<Par, InterpreterError> {
        match self.eval_drive(EvWork::EEval(par), env)? {
            EvVal::Par(p) => Ok(p),
            _ => unreachable!("eval_expr: drive produced non-Par"),
        }
    }
    pub fn eval_expr_to_par(&self, expr: &Expr, env: &Env<Par>) -> Result<Par, InterpreterError> {
        match self.eval_drive(EvWork::EToPar(expr), env)? {
            EvVal::Par(p) => Ok(p),
            _ => unreachable!("eval_expr_to_par: drive produced non-Par"),
        }
    }
    // Kept for SCC API symmetry (the six evaluators are all thin wrappers). In
    // PRODUCTION the drive reaches `EToExpr` internally (descend_to_expr), so no
    // caller invokes this wrapper directly; the differential harness DOES call it
    // (vs `eval_expr_to_expr_recursive`), so it is live under `cfg(test)`.
    #[cfg_attr(not(test), allow(dead_code))]
    fn eval_expr_to_expr(&self, expr: &Expr, env: &Env<Par>) -> Result<Expr, InterpreterError> {
        match self.eval_drive(EvWork::EToExpr(expr), env)? {
            EvVal::Expr(e) => Ok(e),
            _ => unreachable!("eval_expr_to_expr: drive produced non-Expr"),
        }
    }
    fn eval_single_expr(&self, p: &Par, env: &Env<Par>) -> Result<Expr, InterpreterError> {
        match self.eval_drive(EvWork::ESingle(p), env)? {
            EvVal::Expr(e) => Ok(e),
            _ => unreachable!("eval_single_expr: drive produced non-Expr"),
        }
    }
    fn eval_to_bool(&self, p: &Par, env: &Env<Par>) -> Result<bool, InterpreterError> {
        match self.eval_drive(EvWork::EBool(p), env)? {
            EvVal::Bool(b) => Ok(b),
            _ => unreachable!("eval_to_bool: drive produced non-Bool"),
        }
    }
    fn eval_to_i64(&self, p: &Par, env: &Env<Par>) -> Result<i64, InterpreterError> {
        match self.eval_drive(EvWork::EI64(p), env)? {
            EvVal::I64(i) => Ok(i),
            _ => unreachable!("eval_to_i64: drive produced non-I64"),
        }
    }

    /// Byte-identical shallow equivalent of `par.with_exprs(Vec::new())`: clears
    /// `exprs` and clones every OTHER field. `with_exprs` builds `Par { exprs:
    /// new, ..self.clone() }`, i.e. it deep-clones `exprs` then discards them —
    /// on a D-deep spine that is a D-deep recursive clone (a second SO source and
    /// O(D^2) waste). This produces the identical value without touching `exprs`.
    fn ev_par_shell(par: &Par) -> Par {
        Par {
            exprs: Vec::new(),
            sends: par.sends.clone(),
            receives: par.receives.clone(),
            news: par.news.clone(),
            matches: par.matches.clone(),
            unforgeables: par.unforgeables.clone(),
            bundles: par.bundles.clone(),
            connectives: par.connectives.clone(),
            conditionals: par.conditionals.clone(),
            locally_free: par.locally_free.clone(),
            connective_used: par.connective_used,
        }
    }

    // =======================================================================
    // descend_* handlers: pop one work item; push a leaf value, or (pre-order
    // charge then) push Combine(k) + children reversed, or direct-eval an owned
    // intermediate. One per EvWork non-Combine variant.
    // =======================================================================

    // eval_expr: fold eval_expr_to_par over par.exprs.
    fn descend_eval<'e>(
        &self,
        par: &'e Par,
        _env: &Env<Par>,
        work: &mut Vec<EvWork<'e>>,
        _vals: &mut Vec<EvVal>,
    ) -> Result<(), InterpreterError> {
        work.push(EvWork::Combine(EvKont::Join { par, n: par.exprs.len() }));
        for expr in par.exprs.iter().rev() {
            work.push(EvWork::EToPar(expr));
        }
        Ok(())
    }

    // eval_expr_to_par.
    fn descend_to_par<'e>(
        &self,
        expr: &'e Expr,
        env: &Env<Par>,
        work: &mut Vec<EvWork<'e>>,
        vals: &mut Vec<EvVal>,
    ) -> Result<(), InterpreterError> {
        // Fused method-chain seam (runs FIRST; charges internally, may short-circuit).
        if let Some(ExprInstance::EMethodBody(emethod)) = &expr.expr_instance {
            if let Some(fused) = self.try_eval_fused_method_chain(emethod, env)? {
                vals.push(EvVal::Par(fused));
                return Ok(());
            }
        }
        let expr_instance = match &expr.expr_instance {
            Some(ei) => ei,
            None => {
                return Err(InterpreterError::UndefinedRequiredProtobufFieldError(format!(
                    "{:?}",
                    std::any::type_name::<ExprInstance>()
                )))
            }
        };
        match expr_instance {
            ExprInstance::EVarBody(evar) => {
                // eval_var (charges var_eval_cost) then re-eval via eval_expr (direct).
                let p = self.eval_var(&unwrap_option_safe(evar.v.clone())?, env)?;
                let evaled_p = self.eval_expr(&p, env)?;
                vals.push(EvVal::Par(evaled_p));
                Ok(())
            }
            ExprInstance::EMethodBody(emethod) => {
                self.metering.reserve_primitive(method_call_cost())?;
                // Reproduce `unwrap_option_safe(emethod.target.clone())?`'s None error
                // (type_name::<Par>) without cloning the (possibly deep) target.
                let target = match emethod.target.as_ref() {
                    Some(t) => t,
                    None => {
                        return Err(InterpreterError::UndefinedRequiredProtobufFieldError(format!(
                            "{:?}",
                            std::any::type_name::<Par>()
                        )))
                    }
                };
                work.push(EvWork::Combine(EvKont::ToParMethod {
                    emethod,
                    argc: emethod.arguments.len(),
                }));
                for arg in emethod.arguments.iter().rev() {
                    work.push(EvWork::EEval(arg));
                }
                work.push(EvWork::EEval(target));
                Ok(())
            }
            _ => {
                work.push(EvWork::Combine(EvKont::ToParWrap));
                work.push(EvWork::EToExpr(expr));
                Ok(())
            }
        }
    }

    // eval_single_expr.
    fn descend_single<'e>(
        &self,
        p: &'e Par,
        _env: &Env<Par>,
        work: &mut Vec<EvWork<'e>>,
        _vals: &mut Vec<EvVal>,
    ) -> Result<(), InterpreterError> {
        if !p.sends.is_empty()
            || !p.receives.is_empty()
            || !p.news.is_empty()
            || !p.matches.is_empty()
            || !p.unforgeables.is_empty()
            || !p.bundles.is_empty()
        {
            return Err(InterpreterError::ReduceError(String::from(
                "Error: parallel or non expression found where expression expected.",
            )));
        }
        match p.exprs.as_slice() {
            [e] => {
                // The EToExpr value IS the eval_single_expr result (identity).
                work.push(EvWork::EToExpr(e));
                Ok(())
            }
            _ => Err(InterpreterError::ReduceError(
                "Error: Multiple expressions given.".to_string(),
            )),
        }
    }

    // eval_to_bool.
    fn descend_bool<'e>(
        &self,
        p: &'e Par,
        env: &Env<Par>,
        work: &mut Vec<EvWork<'e>>,
        vals: &mut Vec<EvVal>,
    ) -> Result<(), InterpreterError> {
        if !p.sends.is_empty()
            && !p.receives.is_empty()
            && !p.news.is_empty()
            && !p.matches.is_empty()
            && !p.unforgeables.is_empty()
            && !p.bundles.is_empty()
        {
            return Err(InterpreterError::ReduceError(String::from(
                "Error: parallel or non expression found where expression expected.",
            )));
        }
        match p.exprs.as_slice() {
            [Expr { expr_instance: Some(ExprInstance::GBool(b)) }] => {
                vals.push(EvVal::Bool(*b));
                Ok(())
            }
            [Expr { expr_instance: Some(ExprInstance::EVarBody(EVar { v })) }] => {
                let pv = self.eval_var(&unwrap_option_safe(v.clone())?, env)?;
                let b = self.eval_to_bool(&pv, env)?;
                vals.push(EvVal::Bool(b));
                Ok(())
            }
            [e] => {
                work.push(EvWork::Combine(EvKont::BoolExtract));
                work.push(EvWork::EToExpr(e));
                Ok(())
            }
            _ => Err(InterpreterError::ReduceError(
                "Error: Multiple expressions given.".to_string(),
            )),
        }
    }

    // eval_to_i64.
    fn descend_i64<'e>(
        &self,
        p: &'e Par,
        env: &Env<Par>,
        work: &mut Vec<EvWork<'e>>,
        vals: &mut Vec<EvVal>,
    ) -> Result<(), InterpreterError> {
        if !p.sends.is_empty()
            && !p.receives.is_empty()
            && !p.news.is_empty()
            && !p.matches.is_empty()
            && !p.unforgeables.is_empty()
            && !p.bundles.is_empty()
        {
            return Err(InterpreterError::ReduceError(String::from(
                "Error: parallel or non expression found where expression expected.",
            )));
        }
        match p.exprs.as_slice() {
            [Expr { expr_instance: Some(ExprInstance::GInt(v)) }] => {
                vals.push(EvVal::I64(*v));
                Ok(())
            }
            [Expr { expr_instance: Some(ExprInstance::EVarBody(EVar { v })) }] => {
                let pv = self.eval_var(&unwrap_option_safe(v.clone())?, env)?;
                let i = self.eval_to_i64(&pv, env)?;
                vals.push(EvVal::I64(i));
                Ok(())
            }
            [e] => {
                work.push(EvWork::Combine(EvKont::I64Extract));
                work.push(EvWork::EToExpr(e));
                Ok(())
            }
            _ => Err(InterpreterError::ReduceError(
                "Error: Integer expected, or unimplemented expression.".to_string(),
            )),
        }
    }

    // eval_expr_to_expr — the ~35-arm dispatcher.
    fn descend_to_expr<'e>(
        &self,
        expr: &'e Expr,
        env: &Env<Par>,
        work: &mut Vec<EvWork<'e>>,
        vals: &mut Vec<EvVal>,
    ) -> Result<(), InterpreterError> {
        let expr_instance = match &expr.expr_instance {
            Some(ei) => ei,
            None => {
                return Err(InterpreterError::ReduceError(format!(
                    "Unimplemented expression: {:?}",
                    expr
                )))
            }
        };
        match expr_instance {
            // ---- ground leaves (no charge, no children) ----
            ExprInstance::GBool(x) => {
                vals.push(EvVal::Expr(Expr { expr_instance: Some(ExprInstance::GBool(*x)) }));
                Ok(())
            }
            ExprInstance::GInt(x) => {
                vals.push(EvVal::Expr(Expr { expr_instance: Some(ExprInstance::GInt(*x)) }));
                Ok(())
            }
            ExprInstance::GString(x) => {
                vals.push(EvVal::Expr(Expr { expr_instance: Some(ExprInstance::GString(x.clone())) }));
                Ok(())
            }
            ExprInstance::GUri(x) => {
                vals.push(EvVal::Expr(Expr { expr_instance: Some(ExprInstance::GUri(x.clone())) }));
                Ok(())
            }
            ExprInstance::GByteArray(x) => {
                vals.push(EvVal::Expr(Expr { expr_instance: Some(ExprInstance::GByteArray(x.clone())) }));
                Ok(())
            }
            ExprInstance::GDouble(x) => {
                vals.push(EvVal::Expr(Expr { expr_instance: Some(ExprInstance::GDouble(*x)) }));
                Ok(())
            }
            ExprInstance::GBigInt(x) => {
                vals.push(EvVal::Expr(Expr { expr_instance: Some(ExprInstance::GBigInt(x.clone())) }));
                Ok(())
            }
            ExprInstance::GBigRat(x) => {
                vals.push(EvVal::Expr(Expr { expr_instance: Some(ExprInstance::GBigRat(x.clone())) }));
                Ok(())
            }
            ExprInstance::GFixedPoint(x) => {
                vals.push(EvVal::Expr(Expr { expr_instance: Some(ExprInstance::GFixedPoint(x.clone())) }));
                Ok(())
            }
            ExprInstance::EZipperBody(zipper) => {
                vals.push(EvVal::Expr(Expr {
                    expr_instance: Some(ExprInstance::EZipperBody(zipper.clone())),
                }));
                Ok(())
            }

            // ---- unary ----
            ExprInstance::ENotBody(enot) => {
                work.push(EvWork::Combine(EvKont::Not));
                work.push(EvWork::EBool(enot.p.as_ref().unwrap()));
                Ok(())
            }
            ExprInstance::ENegBody(eneg) => {
                work.push(EvWork::Combine(EvKont::Neg));
                work.push(EvWork::ESingle(eneg.p.as_ref().unwrap()));
                Ok(())
            }

            // ---- binary numeric (ESingle children) ----
            ExprInstance::EMultBody(EMult { p1, p2 }) => {
                work.push(EvWork::Combine(EvKont::Mult));
                work.push(EvWork::ESingle(p2.as_ref().unwrap()));
                work.push(EvWork::ESingle(p1.as_ref().unwrap()));
                Ok(())
            }
            ExprInstance::EDivBody(EDiv { p1, p2 }) => {
                work.push(EvWork::Combine(EvKont::Div));
                work.push(EvWork::ESingle(p2.as_ref().unwrap()));
                work.push(EvWork::ESingle(p1.as_ref().unwrap()));
                Ok(())
            }
            ExprInstance::EModBody(EMod { p1, p2 }) => {
                work.push(EvWork::Combine(EvKont::Mod));
                work.push(EvWork::ESingle(p2.as_ref().unwrap()));
                work.push(EvWork::ESingle(p1.as_ref().unwrap()));
                Ok(())
            }
            ExprInstance::EPlusBody(EPlus { p1, p2 }) => {
                work.push(EvWork::Combine(EvKont::Plus));
                work.push(EvWork::ESingle(p2.as_ref().unwrap()));
                work.push(EvWork::ESingle(p1.as_ref().unwrap()));
                Ok(())
            }
            ExprInstance::EMinusBody(EMinus { p1, p2 }) => {
                work.push(EvWork::Combine(EvKont::Minus));
                work.push(EvWork::ESingle(p2.as_ref().unwrap()));
                work.push(EvWork::ESingle(p1.as_ref().unwrap()));
                Ok(())
            }

            // ---- relational (ESingle children) ----
            ExprInstance::ELtBody(ELt { p1, p2 }) => {
                work.push(EvWork::Combine(EvKont::Relop {
                    relopb: |b1: bool, b2: bool| !b1 & b2,
                    relopi: |i1: i64, i2: i64| i1 < i2,
                    relops: |s1: String, s2: String| s1 < s2,
                }));
                work.push(EvWork::ESingle(p2.as_ref().unwrap()));
                work.push(EvWork::ESingle(p1.as_ref().unwrap()));
                Ok(())
            }
            ExprInstance::ELteBody(ELte { p1, p2 }) => {
                work.push(EvWork::Combine(EvKont::Relop {
                    relopb: |b1: bool, b2: bool| b1 <= b2,
                    relopi: |i1: i64, i2: i64| i1 <= i2,
                    relops: |s1: String, s2: String| s1 <= s2,
                }));
                work.push(EvWork::ESingle(p2.as_ref().unwrap()));
                work.push(EvWork::ESingle(p1.as_ref().unwrap()));
                Ok(())
            }
            ExprInstance::EGtBody(EGt { p1, p2 }) => {
                work.push(EvWork::Combine(EvKont::Relop {
                    relopb: |b1: bool, b2: bool| b1 & !b2,
                    relopi: |i1: i64, i2: i64| i1 > i2,
                    relops: |s1: String, s2: String| s1 > s2,
                }));
                work.push(EvWork::ESingle(p2.as_ref().unwrap()));
                work.push(EvWork::ESingle(p1.as_ref().unwrap()));
                Ok(())
            }
            ExprInstance::EGteBody(EGte { p1, p2 }) => {
                work.push(EvWork::Combine(EvKont::Relop {
                    relopb: |b1: bool, b2: bool| b1 >= b2,
                    relopi: |i1: i64, i2: i64| i1 >= i2,
                    relops: |s1: String, s2: String| s1 >= s2,
                }));
                work.push(EvWork::ESingle(p2.as_ref().unwrap()));
                work.push(EvWork::ESingle(p1.as_ref().unwrap()));
                Ok(())
            }

            // ---- equality (EEval children) ----
            ExprInstance::EEqBody(EEq { p1, p2 }) => {
                work.push(EvWork::Combine(EvKont::Eq));
                work.push(EvWork::EEval(p2.as_ref().unwrap()));
                work.push(EvWork::EEval(p1.as_ref().unwrap()));
                Ok(())
            }
            ExprInstance::ENeqBody(ENeq { p1, p2 }) => {
                work.push(EvWork::Combine(EvKont::Neq));
                work.push(EvWork::EEval(p2.as_ref().unwrap()));
                work.push(EvWork::EEval(p1.as_ref().unwrap()));
                Ok(())
            }

            // ---- boolean (EBool children — both eager, NOT short-circuit) ----
            ExprInstance::EAndBody(EAnd { p1, p2 }) => {
                work.push(EvWork::Combine(EvKont::And));
                work.push(EvWork::EBool(p2.as_ref().unwrap()));
                work.push(EvWork::EBool(p1.as_ref().unwrap()));
                Ok(())
            }
            ExprInstance::EOrBody(EOr { p1, p2 }) => {
                work.push(EvWork::Combine(EvKont::Or));
                work.push(EvWork::EBool(p2.as_ref().unwrap()));
                work.push(EvWork::EBool(p1.as_ref().unwrap()));
                Ok(())
            }

            // ---- matches (EEval target; pattern substituted in combine) ----
            ExprInstance::EMatchesBody(EMatches { target, pattern }) => {
                work.push(EvWork::Combine(EvKont::Matches {
                    pattern: pattern.as_ref().unwrap(),
                }));
                work.push(EvWork::EEval(target.as_ref().unwrap()));
                Ok(())
            }

            // ---- interpolation / append / remove (PRE-order op_call_cost) ----
            ExprInstance::EPercentPercentBody(EPercentPercent { p1, p2 }) => {
                self.metering.reserve_primitive(op_call_cost())?;
                work.push(EvWork::Combine(EvKont::PercentPercent));
                work.push(EvWork::ESingle(p2.as_ref().unwrap()));
                work.push(EvWork::ESingle(p1.as_ref().unwrap()));
                Ok(())
            }
            ExprInstance::EPlusPlusBody(EPlusPlus { p1, p2 }) => {
                self.metering.reserve_primitive(op_call_cost())?;
                work.push(EvWork::Combine(EvKont::PlusPlus));
                work.push(EvWork::ESingle(p2.as_ref().unwrap()));
                work.push(EvWork::ESingle(p1.as_ref().unwrap()));
                Ok(())
            }
            ExprInstance::EMinusMinusBody(EMinusMinus { p1, p2 }) => {
                self.metering.reserve_primitive(op_call_cost())?;
                work.push(EvWork::Combine(EvKont::MinusMinus));
                work.push(EvWork::ESingle(p2.as_ref().unwrap()));
                work.push(EvWork::ESingle(p1.as_ref().unwrap()));
                Ok(())
            }

            // ---- var: eval_var (charge) + re-eval single (direct) ----
            ExprInstance::EVarBody(EVar { v }) => {
                let p = self.eval_var(v.as_ref().unwrap(), env)?;
                let expr_val = self.eval_single_expr(&p, env)?;
                vals.push(EvVal::Expr(expr_val));
                Ok(())
            }

            // ---- collections worklisting their (term-borrowed) elements ----
            ExprInstance::EListBody(e1) => {
                work.push(EvWork::Combine(EvKont::EListK { e1, n: e1.ps.len() }));
                for p in e1.ps.iter().rev() {
                    work.push(EvWork::EEval(p));
                }
                Ok(())
            }
            ExprInstance::ETupleBody(e1) => {
                work.push(EvWork::Combine(EvKont::ETupleK { e1, n: e1.ps.len() }));
                for p in e1.ps.iter().rev() {
                    work.push(EvWork::EEval(p));
                }
                Ok(())
            }
            ExprInstance::EPathmapBody(e1) => {
                work.push(EvWork::Combine(EvKont::EPathmapK { e1, n: e1.ps.len() }));
                for p in e1.ps.iter().rev() {
                    work.push(EvWork::EEval(p));
                }
                Ok(())
            }

            // ---- Set/Map: SORTED owned elements -> direct eval via shared helper ----
            ExprInstance::ESetBody(eset) => {
                let e = self.combine_eset(eset, |q| self.eval_expr(q, env))?;
                vals.push(EvVal::Expr(e));
                Ok(())
            }
            ExprInstance::EMapBody(emap) => {
                let e = self.combine_emap(emap, |q| self.eval_expr(q, env))?;
                vals.push(EvVal::Expr(e));
                Ok(())
            }

            // ---- method: fused-first; else method_call_cost + worklist target+args ----
            ExprInstance::EMethodBody(emethod) => {
                if let Some(fused) = self.try_eval_fused_method_chain(emethod, env)? {
                    let e = self.eval_single_expr(&fused, env)?;
                    vals.push(EvVal::Expr(e));
                    return Ok(());
                }
                self.metering.reserve_primitive(method_call_cost())?;
                work.push(EvWork::Combine(EvKont::EMethodExprK {
                    emethod,
                    argc: emethod.arguments.len(),
                }));
                for arg in emethod.arguments.iter().rev() {
                    work.push(EvWork::EEval(arg));
                }
                work.push(EvWork::EEval(emethod.target.as_ref().unwrap()));
                Ok(())
            }
        }
    }

    // =======================================================================
    // combine: pop child values, run the arm's post-order body (via the shared
    // combine_* helper), push the result value. Re-eval of shallow owned
    // intermediates is dispatched here (trampoline: self.eval_*) so the shared
    // helpers stay pure; the recursive twin passes its *_recursive closures.
    // =======================================================================
    fn combine(&self, k: EvKont, env: &Env<Par>, vals: &mut Vec<EvVal>) -> Result<(), InterpreterError> {
        match k {
            EvKont::Join { par, n } => {
                let children = ev_pop_n_par(vals, n);
                let result = children
                    .into_iter()
                    .fold(Self::ev_par_shell(par), |acc, expr| concatenate_pars(acc, expr));
                vals.push(EvVal::Par(result));
                Ok(())
            }
            EvKont::ToParWrap => {
                let e = ev_pop_expr(vals);
                vals.push(EvVal::Par(Par::default().with_exprs(vec![e])));
                Ok(())
            }
            EvKont::ToParMethod { emethod, argc } => {
                let args = ev_pop_n_par(vals, argc);
                let target = ev_pop_par(vals);
                let result_par = match self.method_table().get(&emethod.method_name) {
                    Some(_method) => _method.apply(target, args, env)?,
                    None => {
                        return Err(InterpreterError::ReduceError(format!(
                            "Unimplemented method: {}",
                            emethod.method_name
                        )))
                    }
                };
                vals.push(EvVal::Par(result_par));
                Ok(())
            }
            EvKont::Neg => {
                let v = ev_pop_expr(vals);
                let e = self.combine_neg(v)?;
                vals.push(EvVal::Expr(e));
                Ok(())
            }
            EvKont::Not => {
                let b = ev_pop_bool(vals);
                vals.push(EvVal::Expr(Expr { expr_instance: Some(ExprInstance::GBool(!b)) }));
                Ok(())
            }
            EvKont::Mult => {
                let v2 = ev_pop_expr(vals);
                let v1 = ev_pop_expr(vals);
                let e = self.combine_mult(v1, v2)?;
                vals.push(EvVal::Expr(e));
                Ok(())
            }
            EvKont::Div => {
                let v2 = ev_pop_expr(vals);
                let v1 = ev_pop_expr(vals);
                let e = self.combine_div(v1, v2)?;
                vals.push(EvVal::Expr(e));
                Ok(())
            }
            EvKont::Mod => {
                let v2 = ev_pop_expr(vals);
                let v1 = ev_pop_expr(vals);
                let e = self.combine_mod(v1, v2)?;
                vals.push(EvVal::Expr(e));
                Ok(())
            }
            EvKont::Plus => {
                let v2 = ev_pop_expr(vals);
                let v1 = ev_pop_expr(vals);
                let e = self.combine_plus(v1, v2, env, |q| self.eval_single_expr(q, env))?;
                vals.push(EvVal::Expr(e));
                Ok(())
            }
            EvKont::Minus => {
                let v2 = ev_pop_expr(vals);
                let v1 = ev_pop_expr(vals);
                let e = self.combine_minus(v1, v2, env, |q| self.eval_single_expr(q, env))?;
                vals.push(EvVal::Expr(e));
                Ok(())
            }
            EvKont::Relop { relopb, relopi, relops } => {
                let v2 = ev_pop_expr(vals);
                let v1 = ev_pop_expr(vals);
                let e = self.combine_relop(v1, v2, relopb, relopi, relops)?;
                vals.push(EvVal::Expr(e));
                Ok(())
            }
            EvKont::Eq => {
                let v2 = ev_pop_par(vals);
                let v1 = ev_pop_par(vals);
                let e = self.combine_eq(v1, v2, env)?;
                vals.push(EvVal::Expr(e));
                Ok(())
            }
            EvKont::Neq => {
                let v2 = ev_pop_par(vals);
                let v1 = ev_pop_par(vals);
                let e = self.combine_neq(v1, v2, env)?;
                vals.push(EvVal::Expr(e));
                Ok(())
            }
            EvKont::And => {
                let b2 = ev_pop_bool(vals);
                let b1 = ev_pop_bool(vals);
                self.metering.reserve_primitive(boolean_and_cost())?;
                vals.push(EvVal::Expr(Expr { expr_instance: Some(ExprInstance::GBool(b1 && b2)) }));
                Ok(())
            }
            EvKont::Or => {
                let b2 = ev_pop_bool(vals);
                let b1 = ev_pop_bool(vals);
                self.metering.reserve_primitive(boolean_or_cost())?;
                vals.push(EvVal::Expr(Expr { expr_instance: Some(ExprInstance::GBool(b1 || b2)) }));
                Ok(())
            }
            EvKont::Matches { pattern } => {
                let evaled_target = ev_pop_par(vals);
                let e = self.combine_matches(evaled_target, pattern, env)?;
                vals.push(EvVal::Expr(e));
                Ok(())
            }
            EvKont::PercentPercent => {
                let v2 = ev_pop_expr(vals);
                let v1 = ev_pop_expr(vals);
                let e = self.combine_percent_percent(v1, v2, |q| self.eval_single_expr(q, env))?;
                vals.push(EvVal::Expr(e));
                Ok(())
            }
            EvKont::PlusPlus => {
                let v2 = ev_pop_expr(vals);
                let v1 = ev_pop_expr(vals);
                let e = self.combine_plus_plus(v1, v2, env, |q| self.eval_single_expr(q, env))?;
                vals.push(EvVal::Expr(e));
                Ok(())
            }
            EvKont::MinusMinus => {
                let v2 = ev_pop_expr(vals);
                let v1 = ev_pop_expr(vals);
                let e = self.combine_minus_minus(v1, v2, env, |q| self.eval_single_expr(q, env))?;
                vals.push(EvVal::Expr(e));
                Ok(())
            }
            EvKont::EListK { e1, n } => {
                let evaled_ps = ev_pop_n_par(vals, n);
                let e = self.combine_elist(evaled_ps, e1)?;
                vals.push(EvVal::Expr(e));
                Ok(())
            }
            EvKont::ETupleK { e1, n } => {
                let evaled_ps = ev_pop_n_par(vals, n);
                let e = self.combine_etuple(evaled_ps, e1)?;
                vals.push(EvVal::Expr(e));
                Ok(())
            }
            EvKont::EPathmapK { e1, n } => {
                let evaled_ps = ev_pop_n_par(vals, n);
                let e = self.combine_epathmap(evaled_ps, e1)?;
                vals.push(EvVal::Expr(e));
                Ok(())
            }
            EvKont::EMethodExprK { emethod, argc } => {
                let args = ev_pop_n_par(vals, argc);
                let target = ev_pop_par(vals);
                let result_par = self.apply_method_expr(emethod, target, args, env)?;
                // Re-eval the apply result (direct; a fresh bounded drive).
                let e = self.eval_single_expr(&result_par, env)?;
                vals.push(EvVal::Expr(e));
                Ok(())
            }
            EvKont::BoolExtract => {
                let evaled = ev_pop_expr(vals);
                let b = Self::extract_bool(evaled)?;
                vals.push(EvVal::Bool(b));
                Ok(())
            }
            EvKont::I64Extract => {
                let evaled = ev_pop_expr(vals);
                let i = Self::extract_i64(evaled)?;
                vals.push(EvVal::I64(i));
                Ok(())
            }
        }
    }

    // ---- eval_to_bool / eval_to_i64 [e]-arm extractors ----
    fn extract_bool(evaled: Expr) -> Result<bool, InterpreterError> {
        match evaled.expr_instance {
            Some(expr_instance) => match expr_instance {
                ExprInstance::GBool(b) => Ok(b),
                _ => Err(InterpreterError::ReduceError(
                    "Error: expression didn't evaluate to boolean.".to_string(),
                )),
            },
            None => Err(InterpreterError::MethodNotDefined {
                method: String::from("expr_instance"),
                other_type: String::from("None"),
            }),
        }
    }
    fn extract_i64(evaled: Expr) -> Result<i64, InterpreterError> {
        match evaled.expr_instance {
            Some(expr_instance) => match expr_instance {
                ExprInstance::GInt(v) => Ok(v),
                _ => Err(InterpreterError::ReduceError(
                    "Error: expression didn't evaluate to integer.".to_string(),
                )),
            },
            None => Err(InterpreterError::MethodNotDefined {
                method: String::from("expr_instance"),
                other_type: String::from("None"),
            }),
        }
    }

    // =======================================================================
    // combine_* helpers: each is the EXACT post-order body of an eval_expr_to_expr
    // arm, with the child sub-values passed in as parameters instead of being
    // produced by `eval_*(child)?`. SHARED by the trampoline `combine` and by the
    // recursive twin dispatcher, so the intricate arithmetic/charge/error logic
    // lives in exactly one place.
    // =======================================================================

    // relop (ELt/ELte/EGt/EGte). v1,v2 already `eval_single_expr`'d.
    fn combine_relop(
        &self,
        v1: Expr,
        v2: Expr,
        relopb: fn(bool, bool) -> bool,
        relopi: fn(i64, i64) -> bool,
        relops: fn(String, String) -> bool,
    ) -> Result<Expr, InterpreterError> {
        match (v1.expr_instance.clone().unwrap(), v2.expr_instance.clone().unwrap()) {
            (ExprInstance::GBool(b1), ExprInstance::GBool(b2)) => {
                self.metering.reserve_primitive(comparison_cost())?;
                Ok(Expr { expr_instance: Some(ExprInstance::GBool(relopb(b1, b2))) })
            }
            (ExprInstance::GInt(i1), ExprInstance::GInt(i2)) => {
                self.metering.reserve_primitive(comparison_cost())?;
                Ok(Expr { expr_instance: Some(ExprInstance::GBool(relopi(i1, i2))) })
            }
            (ExprInstance::GString(s1), ExprInstance::GString(s2)) => {
                self.metering.reserve_primitive(comparison_cost())?;
                Ok(Expr { expr_instance: Some(ExprInstance::GBool(relops(s1, s2))) })
            }
            (ExprInstance::GDouble(d1), ExprInstance::GDouble(d2)) => {
                self.metering.reserve_primitive(comparison_cost())?;
                let f1 = f64::from_bits(d1);
                let f2 = f64::from_bits(d2);
                if f1.is_nan() || f2.is_nan() {
                    Ok(Expr { expr_instance: Some(ExprInstance::GBool(false)) })
                } else {
                    Ok(Expr {
                        expr_instance: Some(ExprInstance::GBool(relopi(
                            f1.partial_cmp(&f2).map_or(0, |o| o as i64),
                            0,
                        ))),
                    })
                }
            }
            (ExprInstance::GBigInt(b1), ExprInstance::GBigInt(b2)) => {
                self.metering.reserve_primitive(bigint_comparison_cost(b1.len(), b2.len()))?;
                let cmp = compare_twos_complement_bytes(&b1, &b2);
                Ok(Expr { expr_instance: Some(ExprInstance::GBool(relopi(cmp as i64, 0))) })
            }
            (ExprInstance::GBigRat(r1), ExprInstance::GBigRat(r2)) => {
                self.metering.reserve_primitive(bigrat_comparison_cost(
                    r1.numerator.len(),
                    r1.denominator.len(),
                    r2.numerator.len(),
                    r2.denominator.len(),
                ))?;
                let cmp = compare_big_rationals(&r1, &r2);
                Ok(Expr { expr_instance: Some(ExprInstance::GBool(relopi(cmp as i64, 0))) })
            }
            (ExprInstance::GFixedPoint(fp1), ExprInstance::GFixedPoint(fp2)) => {
                self.metering.reserve_primitive(bigint_comparison_cost(
                    fp1.unscaled.len(),
                    fp2.unscaled.len(),
                ))?;
                let cmp = compare_fixed_points(&fp1, &fp2)?;
                Ok(Expr { expr_instance: Some(ExprInstance::GBool(relopi(cmp as i64, 0))) })
            }
            _ => Err(InterpreterError::ReduceError(format!(
                "Unexpected compare: {:?} vs. {:?}",
                v1, v2
            ))),
        }
    }

    // ENeg. v already `eval_single_expr`'d.
    fn combine_neg(&self, v: Expr) -> Result<Expr, InterpreterError> {
        match v.expr_instance.unwrap() {
            ExprInstance::GInt(i) => {
                let result = i.checked_neg().ok_or_else(|| {
                    InterpreterError::ReduceError("Arithmetic overflow in negation".to_string())
                })?;
                Ok(Expr { expr_instance: Some(ExprInstance::GInt(result)) })
            }
            ExprInstance::GDouble(bits) => {
                let f = f64::from_bits(bits);
                Ok(Expr { expr_instance: Some(ExprInstance::GDouble((-f).to_bits())) })
            }
            ExprInstance::GBigInt(bytes) => {
                self.metering.reserve_primitive(bigint_negation_cost(bytes.len()))?;
                make_bigint_expr(negate_twos_complement(&bytes), "negation")
            }
            ExprInstance::GBigRat(rat) => {
                self.metering.reserve_primitive(bigrat_negation_cost(rat.numerator.len()))?;
                make_bigrat_expr(
                    models::rhoapi::GBigRational {
                        numerator: negate_twos_complement(&rat.numerator),
                        denominator: rat.denominator,
                    },
                    "negation",
                )
            }
            ExprInstance::GFixedPoint(fp) => {
                self.metering.reserve_primitive(bigint_negation_cost(fp.unscaled.len()))?;
                make_fixedpoint_expr(
                    models::rhoapi::GFixedPoint {
                        unscaled: negate_twos_complement(&fp.unscaled),
                        scale: fp.scale,
                    },
                    "negation",
                )
            }
            other => Err(InterpreterError::OperatorNotDefined {
                op: "neg".to_string(),
                other_type: get_type(other),
            }),
        }
    }

    // EMult. v1,v2 already `eval_single_expr`'d.
    fn combine_mult(&self, v1: Expr, v2: Expr) -> Result<Expr, InterpreterError> {
        match (v1.expr_instance.unwrap(), v2.expr_instance.unwrap()) {
            (ExprInstance::GInt(lhs), ExprInstance::GInt(rhs)) => {
                self.metering.reserve_primitive(multiplication_cost())?;
                let result = lhs.checked_mul(rhs).ok_or_else(|| {
                    InterpreterError::ReduceError("Arithmetic overflow in multiplication".to_string())
                })?;
                Ok(Expr { expr_instance: Some(ExprInstance::GInt(result)) })
            }
            (ExprInstance::GDouble(d1), ExprInstance::GDouble(d2)) => {
                self.metering.reserve_primitive(multiplication_cost())?;
                let result = f64::from_bits(d1) * f64::from_bits(d2);
                Ok(Expr { expr_instance: Some(ExprInstance::GDouble(result.to_bits())) })
            }
            (ExprInstance::GBigInt(b1), ExprInstance::GBigInt(b2)) => {
                self.metering.reserve_primitive(bigint_multiplication_cost(b1.len(), b2.len()))?;
                make_bigint_expr(multiply_twos_complement(&b1, &b2), "multiplication")
            }
            (ExprInstance::GBigRat(r1), ExprInstance::GBigRat(r2)) => {
                self.metering.reserve_primitive(bigrat_multiplication_cost(
                    r1.numerator.len(),
                    r1.denominator.len(),
                    r2.numerator.len(),
                    r2.denominator.len(),
                ))?;
                make_bigrat_expr(multiply_big_rationals(&r1, &r2), "multiplication")
            }
            (ExprInstance::GFixedPoint(fp1), ExprInstance::GFixedPoint(fp2)) => {
                if fp1.scale != fp2.scale {
                    return Err(InterpreterError::OperatorExpectedError {
                        op: "*".to_string(),
                        expected: format!("FixedPoint(p{})", fp1.scale),
                        other_type: format!("FixedPoint(p{})", fp2.scale),
                    });
                }
                self.metering.reserve_primitive(bigint_multiplication_cost(
                    fp1.unscaled.len(),
                    fp2.unscaled.len(),
                ))?;
                make_fixedpoint_expr(multiply_fixed_points(&fp1, &fp2), "multiplication")
            }
            (lhs, rhs) => {
                let lhs_type = get_type(lhs);
                let rhs_type = get_type(rhs);
                if lhs_type == rhs_type {
                    Err(InterpreterError::OperatorNotDefined { op: "*".to_string(), other_type: lhs_type })
                } else {
                    Err(InterpreterError::OperatorExpectedError {
                        op: "*".to_string(),
                        expected: lhs_type,
                        other_type: rhs_type,
                    })
                }
            }
        }
    }

    // EDiv. v1,v2 already `eval_single_expr`'d.
    fn combine_div(&self, v1: Expr, v2: Expr) -> Result<Expr, InterpreterError> {
        match (v1.expr_instance.unwrap(), v2.expr_instance.unwrap()) {
            (ExprInstance::GInt(lhs), ExprInstance::GInt(rhs)) => {
                self.metering.reserve_primitive(division_cost())?;
                if rhs == 0 {
                    return Err(InterpreterError::ReduceError(
                        "Division by zero".to_string(),
                    ));
                }
                if lhs == i64::MIN && rhs == -1 {
                    return Err(InterpreterError::ReduceError(
                        "Arithmetic overflow in division".to_string(),
                    ));
                }
                Ok(Expr {
                    expr_instance: Some(ExprInstance::GInt(lhs / rhs)),
                })
            }
            (ExprInstance::GDouble(d1), ExprInstance::GDouble(d2)) => {
                self.metering.reserve_primitive(division_cost())?;
                let result = f64::from_bits(d1) / f64::from_bits(d2);
                Ok(Expr {
                    expr_instance: Some(ExprInstance::GDouble(result.to_bits())),
                })
            }
            (ExprInstance::GBigInt(b1), ExprInstance::GBigInt(b2)) => {
                self.metering
                    .reserve_primitive(bigint_division_cost(b1.len(), b2.len()))?;
                if is_zero_twos_complement(&b2) {
                    return Err(InterpreterError::ReduceError(
                        "Division by zero".to_string(),
                    ));
                }
                make_bigint_expr(divide_twos_complement(&b1, &b2), "division")
            }
            (ExprInstance::GBigRat(r1), ExprInstance::GBigRat(r2)) => {
                self.metering.reserve_primitive(bigrat_division_cost(
                    r1.numerator.len(),
                    r1.denominator.len(),
                    r2.numerator.len(),
                    r2.denominator.len(),
                ))?;
                if is_zero_twos_complement(&r2.numerator) {
                    return Err(InterpreterError::ReduceError(
                        "Division by zero".to_string(),
                    ));
                }
                make_bigrat_expr(divide_big_rationals(&r1, &r2), "division")
            }
            (ExprInstance::GFixedPoint(fp1), ExprInstance::GFixedPoint(fp2)) => {
                if fp1.scale != fp2.scale {
                    return Err(InterpreterError::OperatorExpectedError {
                        op: "/".to_string(),
                        expected: format!("FixedPoint(p{})", fp1.scale),
                        other_type: format!("FixedPoint(p{})", fp2.scale),
                    });
                }
                self.metering.reserve_primitive(bigint_division_cost(
                    fp1.unscaled.len(),
                    fp2.unscaled.len(),
                ))?;
                if is_zero_twos_complement(&fp2.unscaled) {
                    return Err(InterpreterError::ReduceError(
                        "Division by zero".to_string(),
                    ));
                }
                make_fixedpoint_expr(divide_fixed_points(&fp1, &fp2), "division")
            }
            (lhs, rhs) => {
                let lhs_type = get_type(lhs);
                let rhs_type = get_type(rhs);
                if lhs_type == rhs_type {
                    Err(InterpreterError::OperatorNotDefined {
                        op: "/".to_string(),
                        other_type: lhs_type,
                    })
                } else {
                    Err(InterpreterError::OperatorExpectedError {
                        op: "/".to_string(),
                        expected: lhs_type,
                        other_type: rhs_type,
                    })
                }
            }
        }
    }

    // EMod. v1,v2 already `eval_single_expr`'d.
    fn combine_mod(&self, v1: Expr, v2: Expr) -> Result<Expr, InterpreterError> {
        match (v1.expr_instance.unwrap(), v2.expr_instance.unwrap()) {
            (ExprInstance::GInt(lhs), ExprInstance::GInt(rhs)) => {
                self.metering.reserve_primitive(modulo_cost())?;
                if rhs == 0 {
                    return Err(InterpreterError::ReduceError(
                        "Modulo by zero".to_string(),
                    ));
                }
                if lhs == i64::MIN && rhs == -1 {
                    return Err(InterpreterError::ReduceError(
                        "Arithmetic overflow in modulo".to_string(),
                    ));
                }
                Ok(Expr {
                    expr_instance: Some(ExprInstance::GInt(lhs % rhs)),
                })
            }
            (ExprInstance::GDouble(_), ExprInstance::GDouble(_)) => {
                Err(InterpreterError::ReduceError(
                    "Modulus not defined on floating point".to_string(),
                ))
            }
            (ExprInstance::GBigInt(b1), ExprInstance::GBigInt(b2)) => {
                self.metering
                    .reserve_primitive(bigint_modulo_cost(b1.len(), b2.len()))?;
                if is_zero_twos_complement(&b2) {
                    return Err(InterpreterError::ReduceError(
                        "Modulo by zero".to_string(),
                    ));
                }
                make_bigint_expr(modulo_twos_complement(&b1, &b2), "%")
            }
            (ExprInstance::GBigRat(_), ExprInstance::GBigRat(r2)) => {
                if is_zero_twos_complement(&r2.numerator) {
                    return Err(InterpreterError::ReduceError(
                        "Modulo by zero".to_string(),
                    ));
                }
                Ok(Expr {
                    expr_instance: Some(ExprInstance::GBigRat(
                        models::rhoapi::GBigRational {
                            numerator: vec![0],
                            denominator: vec![1],
                        },
                    )),
                })
            }
            (ExprInstance::GFixedPoint(fp1), ExprInstance::GFixedPoint(fp2)) => {
                if fp1.scale != fp2.scale {
                    return Err(InterpreterError::OperatorExpectedError {
                        op: "%".to_string(),
                        expected: format!("FixedPoint(p{})", fp1.scale),
                        other_type: format!("FixedPoint(p{})", fp2.scale),
                    });
                }
                self.metering.reserve_primitive(bigint_modulo_cost(
                    fp1.unscaled.len(),
                    fp2.unscaled.len(),
                ))?;
                if is_zero_twos_complement(&fp2.unscaled) {
                    return Err(InterpreterError::ReduceError(
                        "Modulo by zero".to_string(),
                    ));
                }
                let ua = bytes_to_bigint(&fp1.unscaled);
                let ub = bytes_to_bigint(&fp2.unscaled);
                let remainder = &ua % &ub;
                make_fixedpoint_expr(
                    models::rhoapi::GFixedPoint {
                        unscaled: bigint_to_bytes(&remainder),
                        scale: fp1.scale,
                    },
                    "%",
                )
            }
            (lhs, rhs) => {
                let lhs_type = get_type(lhs);
                let rhs_type = get_type(rhs);
                if lhs_type == rhs_type {
                    Err(InterpreterError::OperatorNotDefined {
                        op: "%".to_string(),
                        other_type: lhs_type,
                    })
                } else {
                    Err(InterpreterError::OperatorExpectedError {
                        op: "%".to_string(),
                        expected: lhs_type,
                        other_type: rhs_type,
                    })
                }
            }
        }
    }

    // EPlus. v1,v2 already `eval_single_expr`'d; ESet sub-arm re-evals via `eval_single`.
    fn combine_plus<F: Fn(&Par) -> Result<Expr, InterpreterError>>(
        &self, v1: Expr, v2: Expr, env: &Env<Par>, eval_single: F,
    ) -> Result<Expr, InterpreterError> {
        match (v1.expr_instance.unwrap(), v2.expr_instance.unwrap()) {
            (ExprInstance::GInt(lhs), ExprInstance::GInt(rhs)) => {
                self.metering.reserve_primitive(sum_cost())?;
                Ok(Expr {
                    expr_instance: Some(ExprInstance::GInt(lhs.wrapping_add(rhs))),
                })
            }

            (ExprInstance::GDouble(d1), ExprInstance::GDouble(d2)) => {
                self.metering.reserve_primitive(sum_cost())?;
                let result = f64::from_bits(d1) + f64::from_bits(d2);
                Ok(Expr {
                    expr_instance: Some(ExprInstance::GDouble(result.to_bits())),
                })
            }

            (ExprInstance::GBigInt(b1), ExprInstance::GBigInt(b2)) => {
                self.metering
                    .reserve_primitive(bigint_sum_cost(b1.len(), b2.len()))?;
                make_bigint_expr(add_twos_complement(&b1, &b2), "+")
            }

            (ExprInstance::GBigRat(r1), ExprInstance::GBigRat(r2)) => {
                self.metering.reserve_primitive(bigrat_sum_cost(
                    r1.numerator.len(),
                    r1.denominator.len(),
                    r2.numerator.len(),
                    r2.denominator.len(),
                ))?;
                make_bigrat_expr(add_big_rationals(&r1, &r2), "+")
            }

            (ExprInstance::GFixedPoint(fp1), ExprInstance::GFixedPoint(fp2)) => {
                if fp1.scale != fp2.scale {
                    return Err(InterpreterError::OperatorExpectedError {
                        op: "+".to_string(),
                        expected: format!("FixedPoint(p{})", fp1.scale),
                        other_type: format!("FixedPoint(p{})", fp2.scale),
                    });
                }
                self.metering.reserve_primitive(bigint_sum_cost(
                    fp1.unscaled.len(),
                    fp2.unscaled.len(),
                ))?;
                make_fixedpoint_expr(
                    models::rhoapi::GFixedPoint {
                        unscaled: add_twos_complement(&fp1.unscaled, &fp2.unscaled),
                        scale: fp1.scale,
                    },
                    "+",
                )
            }

            (ExprInstance::ESetBody(lhs), rhs) => {
                self.metering.reserve_primitive(op_call_cost())?;
                let result_par = self.add_method().apply(
                    Par::default().with_exprs(vec![Expr {
                        expr_instance: Some(ExprInstance::ESetBody(lhs)),
                    }]),
                    vec![Par::default().with_exprs(vec![Expr {
                        expr_instance: Some(rhs),
                    }])],
                    env,
                )?;

                let result_expr = eval_single(&result_par)?;
                Ok(result_expr)
            }

            (ExprInstance::GInt(_), other)
            | (ExprInstance::GDouble(_), other)
            | (ExprInstance::GBigInt(_), other)
            | (ExprInstance::GBigRat(_), other)
            | (ExprInstance::GFixedPoint(_), other) => {
                Err(InterpreterError::OperatorExpectedError {
                    op: "+".to_string(),
                    expected: "matching numeric types".to_string(),
                    other_type: get_type(other),
                })
            }

            (other, _) => Err(InterpreterError::OperatorNotDefined {
                op: "+".to_string(),
                other_type: get_type(other),
            }),
        }
    }

    // EMinus. v1,v2 already `eval_single_expr`'d; Map/Set sub-arms re-eval via `eval_single`.
    fn combine_minus<F: Fn(&Par) -> Result<Expr, InterpreterError>>(
        &self, v1: Expr, v2: Expr, env: &Env<Par>, eval_single: F,
    ) -> Result<Expr, InterpreterError> {
        match (v1.expr_instance.unwrap(), v2.expr_instance.unwrap()) {
            (ExprInstance::GInt(lhs), ExprInstance::GInt(rhs)) => {
                self.metering.reserve_primitive(subtraction_cost())?;
                Ok(Expr {
                    expr_instance: Some(ExprInstance::GInt(lhs.wrapping_sub(rhs))),
                })
            }

            (ExprInstance::GDouble(d1), ExprInstance::GDouble(d2)) => {
                self.metering.reserve_primitive(subtraction_cost())?;
                let result = f64::from_bits(d1) - f64::from_bits(d2);
                Ok(Expr {
                    expr_instance: Some(ExprInstance::GDouble(result.to_bits())),
                })
            }

            (ExprInstance::GBigInt(b1), ExprInstance::GBigInt(b2)) => {
                self.metering
                    .reserve_primitive(bigint_subtraction_cost(b1.len(), b2.len()))?;
                make_bigint_expr(subtract_twos_complement(&b1, &b2), "-")
            }

            (ExprInstance::GBigRat(r1), ExprInstance::GBigRat(r2)) => {
                self.metering.reserve_primitive(bigrat_subtraction_cost(
                    r1.numerator.len(),
                    r1.denominator.len(),
                    r2.numerator.len(),
                    r2.denominator.len(),
                ))?;
                make_bigrat_expr(subtract_big_rationals(&r1, &r2), "-")
            }

            (ExprInstance::GFixedPoint(fp1), ExprInstance::GFixedPoint(fp2)) => {
                if fp1.scale != fp2.scale {
                    return Err(InterpreterError::OperatorExpectedError {
                        op: "-".to_string(),
                        expected: format!("FixedPoint(p{})", fp1.scale),
                        other_type: format!("FixedPoint(p{})", fp2.scale),
                    });
                }
                self.metering.reserve_primitive(bigint_subtraction_cost(
                    fp1.unscaled.len(),
                    fp2.unscaled.len(),
                ))?;
                make_fixedpoint_expr(
                    models::rhoapi::GFixedPoint {
                        unscaled: subtract_twos_complement(
                            &fp1.unscaled,
                            &fp2.unscaled,
                        ),
                        scale: fp1.scale,
                    },
                    "-",
                )
            }

            (ExprInstance::EMapBody(lhs), rhs) => {
                self.metering.reserve_primitive(op_call_cost())?;
                let result_par = self.delete_method().apply(
                    Par::default().with_exprs(vec![Expr {
                        expr_instance: Some(ExprInstance::EMapBody(lhs)),
                    }]),
                    vec![Par::default().with_exprs(vec![Expr {
                        expr_instance: Some(rhs),
                    }])],
                    env,
                )?;

                let result_expr = eval_single(&result_par)?;
                Ok(result_expr)
            }

            (ExprInstance::ESetBody(lhs), rhs) => {
                self.metering.reserve_primitive(op_call_cost())?;
                let result_par = self.delete_method().apply(
                    Par::default().with_exprs(vec![Expr {
                        expr_instance: Some(ExprInstance::ESetBody(lhs)),
                    }]),
                    vec![Par::default().with_exprs(vec![Expr {
                        expr_instance: Some(rhs),
                    }])],
                    env,
                )?;

                let result_expr = eval_single(&result_par)?;
                Ok(result_expr)
            }

            (ExprInstance::GInt(_), other)
            | (ExprInstance::GDouble(_), other)
            | (ExprInstance::GBigInt(_), other)
            | (ExprInstance::GBigRat(_), other)
            | (ExprInstance::GFixedPoint(_), other) => {
                Err(InterpreterError::OperatorExpectedError {
                    op: "-".to_string(),
                    expected: "matching numeric types".to_string(),
                    other_type: get_type(other),
                })
            }

            (other, _) => Err(InterpreterError::OperatorNotDefined {
                op: "-".to_string(),
                other_type: get_type(other),
            }),
        }
    }

    // EEq. v1,v2 already `eval_expr`'d (substitution + NaN-aware compare; substitute is NOT SCC).
    fn combine_eq(&self, v1: Par, v2: Par, env: &Env<Par>) -> Result<Expr, InterpreterError> {
        // TODO: build an equality operator that takes in an environment. - OLD
        let sv1 = self.substitute.substitute_and_charge(&v1, 0, env)?;
        let sv2 = self.substitute.substitute_and_charge(&v2, 0, env)?;
        self.metering
            .reserve_primitive(equality_check_cost(&sv1, &sv2))?;

        let result = if par_contains_nan_double(&sv1) || par_contains_nan_double(&sv2) {
            false
        } else {
            sv1 == sv2
        };
        Ok(Expr {
            expr_instance: Some(ExprInstance::GBool(result)),
        })
    }

    // ENeq.
    fn combine_neq(&self, v1: Par, v2: Par, env: &Env<Par>) -> Result<Expr, InterpreterError> {
        let sv1 = self.substitute.substitute_and_charge(&v1, 0, env)?;
        let sv2 = self.substitute.substitute_and_charge(&v2, 0, env)?;
        self.metering
            .reserve_primitive(equality_check_cost(&sv1, &sv2))?;

        let result = if par_contains_nan_double(&sv1) || par_contains_nan_double(&sv2) {
            true
        } else {
            sv1 != sv2
        };
        Ok(Expr {
            expr_instance: Some(ExprInstance::GBool(result)),
        })
    }

    // EMatches. target already `eval_expr`'d; pattern is the borrowed &Par.
    fn combine_matches(&self, evaled_target: Par, pattern: &Par, env: &Env<Par>) -> Result<Expr, InterpreterError> {
        let subst_target =
            self.substitute
                .substitute_and_charge(&evaled_target, 0, env)?;
        let subst_pattern =
            self.substitute
                .substitute_and_charge(pattern, 1, env)?;

        let mut spatial_matcher = SpatialMatcherContext::new();
        let match_result =
            spatial_matcher.spatial_match_result(subst_target, subst_pattern);

        Ok(Expr {
            expr_instance: Some(ExprInstance::GBool(match_result.is_some())),
        })
    }

    // EPercentPercent (%%). op_call_cost is charged PRE (in descend). v1,v2 `eval_single_expr`'d;
    // map contents re-eval via `eval_single`.
    fn combine_percent_percent<F: Fn(&Par) -> Result<Expr, InterpreterError>>(
        &self, v1: Expr, v2: Expr, eval_single: F,
    ) -> Result<Expr, InterpreterError> {
        fn eval_to_string_pair(
            key_expr: Expr,
            value_expr: Expr,
        ) -> Result<(String, String), InterpreterError> {
            match (
                key_expr.expr_instance.unwrap(),
                value_expr.expr_instance.unwrap(),
            ) {
                (
                    ExprInstance::GString(key_string),
                    ExprInstance::GString(value_string),
                ) => Ok((key_string, value_string)),

                (ExprInstance::GString(key_string), ExprInstance::GInt(value_int)) => {
                    Ok((key_string, value_int.to_string()))
                }

                (
                    ExprInstance::GString(key_string),
                    ExprInstance::GBool(value_bool),
                ) => Ok((key_string, value_bool.to_string())),

                (ExprInstance::GString(key_string), ExprInstance::GUri(uri)) => {
                    Ok((key_string, uri))
                }

                // TODO: Add cases for other ground terms as well? Maybe it would be better
                // to implement cats.Show for all ground terms. - OLD
                (ExprInstance::GString(_), value) => {
                    Err(InterpreterError::ReduceError(format!(
                        "Error: interpolation doesn't support {:?}",
                        get_type(value),
                    )))
                }

                _ => Err(InterpreterError::ReduceError(
                    "Error: interpolation Map should only contain String keys"
                        .to_string(),
                )),
            }
        }

        fn interpolate(string: &str, key_value_pairs: &[(String, String)]) -> String {
            let mut result = String::new();
            let mut current = string.to_string();

            while !current.is_empty() {
                let mut found = false;

                for (k, v) in key_value_pairs {
                    if current.starts_with(&format!("${{{}}}", k)) {
                        result.push_str(v);
                        current = current.split_at(k.len() + 3).1.to_string();
                        found = true;

                        break;
                    }
                }

                if !found {
                    result.push(current.chars().next().unwrap());
                    current.remove(0);
                }
            }

            result
        }
        match (v1.expr_instance.unwrap(), v2.expr_instance.unwrap()) {
            (ExprInstance::GString(lhs), ExprInstance::EMapBody(emap)) => {
                let rhs = ParMapTypeMapper::emap_to_par_map(emap).ps;
                if !lhs.is_empty() || !rhs.is_empty() {
                    let key_value_pairs = rhs
                        .clone()
                        .into_iter()
                        .map(|(k, v)| {
                            let key_expr = eval_single(&k)?;
                            let value_expr = eval_single(&v)?;
                            let result = eval_to_string_pair(key_expr, value_expr)?;
                            Ok(result)
                        })
                        .collect::<Result<Vec<_>, InterpreterError>>()?;

                    self.metering
                        .reserve_incremental_primitive(interpolate_cost(
                            lhs.len() as i64,
                            rhs.length() as i64,
                        ))?;

                    Ok(Expr {
                        expr_instance: Some(ExprInstance::GString(interpolate(
                            &lhs,
                            &key_value_pairs,
                        ))),
                    })
                } else {
                    Ok(Expr {
                        expr_instance: Some(ExprInstance::GString(lhs)),
                    })
                }
            }

            (ExprInstance::GString(_), other) => {
                Err(InterpreterError::OperatorExpectedError {
                    op: "%%".to_string(),
                    expected: String::from("Map"),
                    other_type: get_type(other),
                })
            }

            (other, _) => Err(InterpreterError::OperatorNotDefined {
                op: String::from("%%"),
                other_type: get_type(other),
            }),
        }
    }

    // EPlusPlus (++). op_call_cost PRE. Map/Set union sub-arms re-eval via `eval_single`.
    fn combine_plus_plus<F: Fn(&Par) -> Result<Expr, InterpreterError>>(
        &self, v1: Expr, v2: Expr, env: &Env<Par>, eval_single: F,
    ) -> Result<Expr, InterpreterError> {
        match (v1.expr_instance.unwrap(), v2.expr_instance.unwrap()) {
            (ExprInstance::GString(lhs), ExprInstance::GString(rhs)) => {
                self.metering
                    .reserve_incremental_primitive(string_append_cost(
                        lhs.len() as i64,
                        rhs.len() as i64,
                    ))?;
                Ok(Expr {
                    expr_instance: Some(ExprInstance::GString(lhs + &rhs)),
                })
            }

            (ExprInstance::GByteArray(lhs), ExprInstance::GByteArray(rhs)) => {
                self.metering
                    .reserve_incremental_primitive(byte_array_append_cost(
                        lhs.clone(),
                    ))?;
                Ok(Expr {
                    expr_instance: Some(ExprInstance::GByteArray(
                        lhs.into_iter().chain(rhs.into_iter()).collect(),
                    )),
                })
            }

            (ExprInstance::EListBody(lhs), ExprInstance::EListBody(rhs)) => {
                self.metering
                    .reserve_incremental_primitive(list_append_cost(lhs.clone().ps))?;
                Ok(Expr {
                    expr_instance: Some(ExprInstance::EListBody(EList {
                        ps: lhs.ps.into_iter().chain(rhs.ps.into_iter()).collect(),
                        locally_free: union(lhs.locally_free, rhs.locally_free),
                        connective_used: lhs.connective_used || rhs.connective_used,
                        remainder: None,
                    })),
                })
            }

            (ExprInstance::EMapBody(lhs), ExprInstance::EMapBody(rhs)) => {
                let result_par = self.union_method().apply(
                    Par::default().with_exprs(vec![Expr {
                        expr_instance: Some(ExprInstance::EMapBody(lhs)),
                    }]),
                    vec![Par::default().with_exprs(vec![Expr {
                        expr_instance: Some(ExprInstance::EMapBody(rhs)),
                    }])],
                    env,
                )?;
                let result_expr = eval_single(&result_par)?;
                Ok(result_expr)
            }

            (ExprInstance::ESetBody(lhs), ExprInstance::ESetBody(rhs)) => {
                let result_par = self.union_method().apply(
                    Par::default().with_exprs(vec![Expr {
                        expr_instance: Some(ExprInstance::ESetBody(lhs)),
                    }]),
                    vec![Par::default().with_exprs(vec![Expr {
                        expr_instance: Some(ExprInstance::ESetBody(rhs)),
                    }])],
                    env,
                )?;
                let result_expr = eval_single(&result_par)?;
                Ok(result_expr)
            }

            (ExprInstance::GString(_), other) => {
                Err(InterpreterError::OperatorExpectedError {
                    op: "++".to_string(),
                    expected: String::from("String"),
                    other_type: get_type(other),
                })
            }

            (ExprInstance::EListBody(_), other) => {
                Err(InterpreterError::OperatorExpectedError {
                    op: "++".to_string(),
                    expected: String::from("List"),
                    other_type: get_type(other),
                })
            }

            (ExprInstance::EMapBody(_), other) => {
                Err(InterpreterError::OperatorExpectedError {
                    op: "++".to_string(),
                    expected: String::from("Map"),
                    other_type: get_type(other),
                })
            }

            (ExprInstance::ESetBody(_), other) => {
                Err(InterpreterError::OperatorExpectedError {
                    op: "++".to_string(),
                    expected: String::from("Set"),
                    other_type: get_type(other),
                })
            }

            (other, _) => Err(InterpreterError::OperatorNotDefined {
                op: String::from("++"),
                other_type: get_type(other),
            }),
        }
    }

    // EMinusMinus (--). op_call_cost PRE. Set diff sub-arm re-evals via `eval_single`.
    fn combine_minus_minus<F: Fn(&Par) -> Result<Expr, InterpreterError>>(
        &self, v1: Expr, v2: Expr, env: &Env<Par>, eval_single: F,
    ) -> Result<Expr, InterpreterError> {
        match (v1.expr_instance.unwrap(), v2.expr_instance.unwrap()) {
            (ExprInstance::ESetBody(lhs), ExprInstance::ESetBody(rhs)) => {
                let result_par = self.diff_method().apply(
                    Par::default().with_exprs(vec![Expr {
                        expr_instance: Some(ExprInstance::ESetBody(lhs)),
                    }]),
                    vec![Par::default().with_exprs(vec![Expr {
                        expr_instance: Some(ExprInstance::ESetBody(rhs)),
                    }])],
                    env,
                )?;
                let result_expr = eval_single(&result_par)?;
                Ok(result_expr)
            }

            (ExprInstance::ESetBody(_), other) => {
                Err(InterpreterError::OperatorExpectedError {
                    op: "--".to_string(),
                    expected: String::from("Set"),
                    other_type: get_type(other),
                })
            }

            (other, _) => Err(InterpreterError::OperatorNotDefined {
                op: String::from("--"),
                other_type: get_type(other),
            }),
        }
    }

    // EList. evaled_ps already `eval_expr`'d (owned -> no p.clone()).
    fn combine_elist(&self, evaled_ps: Vec<Par>, e1: &EList) -> Result<Expr, InterpreterError> {
        let updated_ps: Vec<Par> = evaled_ps
            .into_iter()
            .map(|p| self.update_locally_free_par(p))
            .collect();

        Ok(Expr {
            expr_instance: Some(ExprInstance::EListBody(
                self.update_locally_free_elist(EList {
                    ps: updated_ps,
                    locally_free: e1.locally_free.clone(),
                    connective_used: e1.connective_used,
                    remainder: None,
                }),
            )),
        })
    }

    // ETuple.
    fn combine_etuple(&self, evaled_ps: Vec<Par>, e1: &ETuple) -> Result<Expr, InterpreterError> {
        let updated_ps: Vec<Par> = evaled_ps
            .into_iter()
            .map(|p| self.update_locally_free_par(p))
            .collect();

        Ok(Expr {
            expr_instance: Some(ExprInstance::ETupleBody(
                self.update_locally_free_etuple(ETuple {
                    ps: updated_ps,
                    locally_free: e1.locally_free.clone(),
                    connective_used: e1.connective_used,
                }),
            )),
        })
    }

    // EPathmap.
    fn combine_epathmap(&self, evaled_ps: Vec<Par>, e1: &EPathMap) -> Result<Expr, InterpreterError> {
        let updated_ps: Vec<Par> = evaled_ps
            .into_iter()
            .map(|p| self.update_locally_free_par(p))
            .collect();

        let rebuilt = EPathMap::new(
            updated_ps,
            e1.locally_free.clone(),
            e1.connective_used,
            None,
        );
        // EPathMap wire: a GROUND eval result canonicalizes to trie
        // order (a PathMap zipper walk, NO sort) so runtime and
        // normalization agree — the same map, however constructed,
        // compares structurally-equal (COMM fires order-insensitively)
        // and hashes to one canonical preimage. A non-ground result
        // is returned unchanged.
        Ok(Expr {
            expr_instance: Some(ExprInstance::EPathmapBody(
                models::rust::pathmap_crate_type_mapper::canonicalize_ground_epathmap(
                    &rebuilt,
                ),
            )),
        })
    }

    // ESet. SORTED owned elements evaluated via `eval_expr` closure.
    fn combine_eset<F: Fn(&Par) -> Result<Par, InterpreterError>>(
        &self, eset: &ESet, eval_expr: F,
    ) -> Result<Expr, InterpreterError> {
        let set = ParSetTypeMapper::eset_to_par_set(eset.clone());
        let evaled_ps = set
            .ps
            .sorted_pars
            .iter()
            .map(|p| eval_expr(p))
            .collect::<Result<Vec<_>, InterpreterError>>()?;

        let updated_ps: Vec<Par> = evaled_ps
            .into_iter()
            .map(|p| self.update_locally_free_par(p))
            .collect();

        let mut cloned_set = set.clone();
        cloned_set.ps = SortedParHashSet::create_from_vec(updated_ps);
        Ok(Expr {
            expr_instance: Some(ExprInstance::ESetBody(
                ParSetTypeMapper::par_set_to_eset(cloned_set),
            )),
        })
    }

    // EMap. SORTED owned key/value pairs via `eval_expr` closure (no update_locally_free_par).
    fn combine_emap<F: Fn(&Par) -> Result<Par, InterpreterError>>(
        &self, emap: &EMap, eval_expr: F,
    ) -> Result<Expr, InterpreterError> {
        let map = ParMapTypeMapper::emap_to_par_map(emap.clone());
        let evaled_ps = map
            .ps
            .clone()
            .into_iter()
            .map(|(k, v)| {
                let e_key = eval_expr(&k)?;
                let e_value = eval_expr(&v)?;
                Ok((e_key, e_value))
            })
            .collect::<Result<Vec<_>, InterpreterError>>()?;

        let mut cloned_map = map.clone();
        cloned_map.ps = SortedParMap::create_from_vec(evaled_ps);
        Ok(Expr {
            expr_instance: Some(ExprInstance::EMapBody(
                ParMapTypeMapper::par_map_to_emap(cloned_map),
            )),
        })
    }

    // EMethod (eval_expr_to_expr site): method_table lookup (Debug error) + apply. The
    // re-eval of the result via `eval_single_expr` is done by the caller (combine / twin).
    fn apply_method_expr(&self, emethod: &EMethod, target_val: Par, arg_vals: Vec<Par>, env: &Env<Par>) -> Result<Par, InterpreterError> {
        let result_par = match self.method_table().get(&emethod.method_name) {
            Some(method_function) => method_function.apply(target_val, arg_vals, env)?,
            None => {
                return Err(InterpreterError::ReduceError(format!(
                    "Unimplemented method: {:?}",
                    emethod.method_name
                )));
            }
        };
        Ok(result_par)
    }


    // =======================================================================
    // RECURSIVE TWIN — the oracle for the differential harness (`differential`
    // module). Each function is a faithful recursive dispatcher over the SAME
    // shared `combine_*` helpers the trampoline uses, so a byte-identical
    // result + charge trace between the two proves the trampoline's descend/
    // combine WIRING (the only new logic). cfg(test): excluded from production.
    // =======================================================================
    #[cfg(test)]
    pub(crate) fn eval_expr_recursive(&self, par: &Par, env: &Env<Par>) -> Result<Par, InterpreterError> {
        let evaled_exprs = par
            .exprs
            .iter()
            .map(|expr| self.eval_expr_to_par_recursive(expr, env))
            .collect::<Result<Vec<_>, InterpreterError>>()?;
        let result = evaled_exprs
            .into_iter()
            .fold(par.with_exprs(Vec::new()), |acc, expr| concatenate_pars(acc, expr));
        Ok(result)
    }

    #[cfg(test)]
    fn eval_expr_to_par_recursive(&self, expr: &Expr, env: &Env<Par>) -> Result<Par, InterpreterError> {
        if let Some(ExprInstance::EMethodBody(emethod)) = &expr.expr_instance {
            if let Some(fused) = self.try_eval_fused_method_chain(emethod, env)? {
                return Ok(fused);
            }
        }
        let expr_instance = match &expr.expr_instance {
            Some(ei) => ei,
            None => {
                return Err(InterpreterError::UndefinedRequiredProtobufFieldError(format!(
                    "{:?}",
                    std::any::type_name::<ExprInstance>()
                )))
            }
        };
        match expr_instance {
            ExprInstance::EVarBody(evar) => {
                let p = self.eval_var(&unwrap_option_safe(evar.v.clone())?, env)?;
                let evaled_p = self.eval_expr_recursive(&p, env)?;
                Ok(evaled_p)
            }
            ExprInstance::EMethodBody(emethod) => {
                self.metering.reserve_primitive(method_call_cost())?;
                let evaled_target =
                    self.eval_expr_recursive(&unwrap_option_safe(emethod.target.clone())?, env)?;
                let evaled_args: Vec<Par> = emethod
                    .arguments
                    .iter()
                    .map(|arg| self.eval_expr_recursive(arg, env))
                    .collect::<Result<Vec<_>, InterpreterError>>()?;
                let result_par = match self.method_table().get(&emethod.method_name) {
                    Some(_method) => _method.apply(evaled_target, evaled_args, env)?,
                    None => {
                        return Err(InterpreterError::ReduceError(format!(
                            "Unimplemented method: {}",
                            emethod.method_name
                        )));
                    }
                };
                Ok(result_par)
            }
            _ => Ok(Par::default().with_exprs(vec![self.eval_expr_to_expr_recursive(expr, env)?])),
        }
    }

    #[cfg(test)]
    fn eval_expr_to_expr_recursive(&self, expr: &Expr, env: &Env<Par>) -> Result<Expr, InterpreterError> {
        match &expr.expr_instance {
            Some(expr_instance) => match expr_instance {
                ExprInstance::GBool(x) => Ok(Expr { expr_instance: Some(ExprInstance::GBool(*x)) }),
                ExprInstance::GInt(x) => Ok(Expr { expr_instance: Some(ExprInstance::GInt(*x)) }),
                ExprInstance::GString(x) => Ok(Expr { expr_instance: Some(ExprInstance::GString(x.clone())) }),
                ExprInstance::GUri(x) => Ok(Expr { expr_instance: Some(ExprInstance::GUri(x.clone())) }),
                ExprInstance::GByteArray(x) => Ok(Expr { expr_instance: Some(ExprInstance::GByteArray(x.clone())) }),
                ExprInstance::GDouble(x) => Ok(Expr { expr_instance: Some(ExprInstance::GDouble(*x)) }),
                ExprInstance::GBigInt(x) => Ok(Expr { expr_instance: Some(ExprInstance::GBigInt(x.clone())) }),
                ExprInstance::GBigRat(x) => Ok(Expr { expr_instance: Some(ExprInstance::GBigRat(x.clone())) }),
                ExprInstance::GFixedPoint(x) => Ok(Expr { expr_instance: Some(ExprInstance::GFixedPoint(x.clone())) }),
                ExprInstance::EZipperBody(zipper) => Ok(Expr { expr_instance: Some(ExprInstance::EZipperBody(zipper.clone())) }),

                ExprInstance::ENotBody(enot) => {
                    let b = self.eval_to_bool_recursive(enot.p.as_ref().unwrap(), env)?;
                    Ok(Expr { expr_instance: Some(ExprInstance::GBool(!b)) })
                }
                ExprInstance::ENegBody(eneg) => {
                    let v = self.eval_single_expr_recursive(eneg.p.as_ref().unwrap(), env)?;
                    self.combine_neg(v)
                }
                ExprInstance::EMultBody(EMult { p1, p2 }) => {
                    let v1 = self.eval_single_expr_recursive(p1.as_ref().unwrap(), env)?;
                    let v2 = self.eval_single_expr_recursive(p2.as_ref().unwrap(), env)?;
                    self.combine_mult(v1, v2)
                }
                ExprInstance::EDivBody(EDiv { p1, p2 }) => {
                    let v1 = self.eval_single_expr_recursive(p1.as_ref().unwrap(), env)?;
                    let v2 = self.eval_single_expr_recursive(p2.as_ref().unwrap(), env)?;
                    self.combine_div(v1, v2)
                }
                ExprInstance::EModBody(EMod { p1, p2 }) => {
                    let v1 = self.eval_single_expr_recursive(p1.as_ref().unwrap(), env)?;
                    let v2 = self.eval_single_expr_recursive(p2.as_ref().unwrap(), env)?;
                    self.combine_mod(v1, v2)
                }
                ExprInstance::EPlusBody(EPlus { p1, p2 }) => {
                    let v1 = self.eval_single_expr_recursive(p1.as_ref().unwrap(), env)?;
                    let v2 = self.eval_single_expr_recursive(p2.as_ref().unwrap(), env)?;
                    self.combine_plus(v1, v2, env, |q| self.eval_single_expr_recursive(q, env))
                }
                ExprInstance::EMinusBody(EMinus { p1, p2 }) => {
                    let v1 = self.eval_single_expr_recursive(p1.as_ref().unwrap(), env)?;
                    let v2 = self.eval_single_expr_recursive(p2.as_ref().unwrap(), env)?;
                    self.combine_minus(v1, v2, env, |q| self.eval_single_expr_recursive(q, env))
                }
                ExprInstance::ELtBody(ELt { p1, p2 }) => {
                    let v1 = self.eval_single_expr_recursive(p1.as_ref().unwrap(), env)?;
                    let v2 = self.eval_single_expr_recursive(p2.as_ref().unwrap(), env)?;
                    self.combine_relop(v1, v2, |b1, b2| !b1 & b2, |i1, i2| i1 < i2, |s1, s2| s1 < s2)
                }
                ExprInstance::ELteBody(ELte { p1, p2 }) => {
                    let v1 = self.eval_single_expr_recursive(p1.as_ref().unwrap(), env)?;
                    let v2 = self.eval_single_expr_recursive(p2.as_ref().unwrap(), env)?;
                    self.combine_relop(v1, v2, |b1, b2| b1 <= b2, |i1, i2| i1 <= i2, |s1, s2| s1 <= s2)
                }
                ExprInstance::EGtBody(EGt { p1, p2 }) => {
                    let v1 = self.eval_single_expr_recursive(p1.as_ref().unwrap(), env)?;
                    let v2 = self.eval_single_expr_recursive(p2.as_ref().unwrap(), env)?;
                    self.combine_relop(v1, v2, |b1, b2| b1 & !b2, |i1, i2| i1 > i2, |s1, s2| s1 > s2)
                }
                ExprInstance::EGteBody(EGte { p1, p2 }) => {
                    let v1 = self.eval_single_expr_recursive(p1.as_ref().unwrap(), env)?;
                    let v2 = self.eval_single_expr_recursive(p2.as_ref().unwrap(), env)?;
                    self.combine_relop(v1, v2, |b1, b2| b1 >= b2, |i1, i2| i1 >= i2, |s1, s2| s1 >= s2)
                }
                ExprInstance::EEqBody(EEq { p1, p2 }) => {
                    let v1 = self.eval_expr_recursive(p1.as_ref().unwrap(), env)?;
                    let v2 = self.eval_expr_recursive(p2.as_ref().unwrap(), env)?;
                    self.combine_eq(v1, v2, env)
                }
                ExprInstance::ENeqBody(ENeq { p1, p2 }) => {
                    let v1 = self.eval_expr_recursive(p1.as_ref().unwrap(), env)?;
                    let v2 = self.eval_expr_recursive(p2.as_ref().unwrap(), env)?;
                    self.combine_neq(v1, v2, env)
                }
                ExprInstance::EAndBody(EAnd { p1, p2 }) => {
                    let b1 = self.eval_to_bool_recursive(p1.as_ref().unwrap(), env)?;
                    let b2 = self.eval_to_bool_recursive(p2.as_ref().unwrap(), env)?;
                    self.metering.reserve_primitive(boolean_and_cost())?;
                    Ok(Expr { expr_instance: Some(ExprInstance::GBool(b1 && b2)) })
                }
                ExprInstance::EOrBody(EOr { p1, p2 }) => {
                    let b1 = self.eval_to_bool_recursive(p1.as_ref().unwrap(), env)?;
                    let b2 = self.eval_to_bool_recursive(p2.as_ref().unwrap(), env)?;
                    self.metering.reserve_primitive(boolean_or_cost())?;
                    Ok(Expr { expr_instance: Some(ExprInstance::GBool(b1 || b2)) })
                }
                ExprInstance::EMatchesBody(EMatches { target, pattern }) => {
                    let evaled_target = self.eval_expr_recursive(target.as_ref().unwrap(), env)?;
                    self.combine_matches(evaled_target, pattern.as_ref().unwrap(), env)
                }
                ExprInstance::EPercentPercentBody(EPercentPercent { p1, p2 }) => {
                    self.metering.reserve_primitive(op_call_cost())?;
                    let v1 = self.eval_single_expr_recursive(p1.as_ref().unwrap(), env)?;
                    let v2 = self.eval_single_expr_recursive(p2.as_ref().unwrap(), env)?;
                    self.combine_percent_percent(v1, v2, |q| self.eval_single_expr_recursive(q, env))
                }
                ExprInstance::EPlusPlusBody(EPlusPlus { p1, p2 }) => {
                    self.metering.reserve_primitive(op_call_cost())?;
                    let v1 = self.eval_single_expr_recursive(p1.as_ref().unwrap(), env)?;
                    let v2 = self.eval_single_expr_recursive(p2.as_ref().unwrap(), env)?;
                    self.combine_plus_plus(v1, v2, env, |q| self.eval_single_expr_recursive(q, env))
                }
                ExprInstance::EMinusMinusBody(EMinusMinus { p1, p2 }) => {
                    self.metering.reserve_primitive(op_call_cost())?;
                    let v1 = self.eval_single_expr_recursive(p1.as_ref().unwrap(), env)?;
                    let v2 = self.eval_single_expr_recursive(p2.as_ref().unwrap(), env)?;
                    self.combine_minus_minus(v1, v2, env, |q| self.eval_single_expr_recursive(q, env))
                }
                ExprInstance::EVarBody(EVar { v }) => {
                    let p = self.eval_var(v.as_ref().unwrap(), env)?;
                    self.eval_single_expr_recursive(&p, env)
                }
                ExprInstance::EListBody(e1) => {
                    let evaled_ps = e1
                        .ps
                        .iter()
                        .map(|p| self.eval_expr_recursive(p, env))
                        .collect::<Result<Vec<_>, InterpreterError>>()?;
                    self.combine_elist(evaled_ps, e1)
                }
                ExprInstance::ETupleBody(e1) => {
                    let evaled_ps = e1
                        .ps
                        .iter()
                        .map(|p| self.eval_expr_recursive(p, env))
                        .collect::<Result<Vec<_>, InterpreterError>>()?;
                    self.combine_etuple(evaled_ps, e1)
                }
                ExprInstance::EPathmapBody(e1) => {
                    let evaled_ps = e1
                        .ps
                        .iter()
                        .map(|p| self.eval_expr_recursive(p, env))
                        .collect::<Result<Vec<_>, InterpreterError>>()?;
                    self.combine_epathmap(evaled_ps, e1)
                }
                ExprInstance::ESetBody(eset) => {
                    self.combine_eset(eset, |q| self.eval_expr_recursive(q, env))
                }
                ExprInstance::EMapBody(emap) => {
                    self.combine_emap(emap, |q| self.eval_expr_recursive(q, env))
                }
                ExprInstance::EMethodBody(emethod) => {
                    if let Some(fused) = self.try_eval_fused_method_chain(emethod, env)? {
                        return self.eval_single_expr_recursive(&fused, env);
                    }
                    self.metering.reserve_primitive(method_call_cost())?;
                    let evaled_target =
                        self.eval_expr_recursive(emethod.target.as_ref().unwrap(), env)?;
                    let evaled_args: Vec<Par> = emethod
                        .arguments
                        .iter()
                        .map(|arg| self.eval_expr_recursive(arg, env))
                        .collect::<Result<Vec<_>, InterpreterError>>()?;
                    let result_par = self.apply_method_expr(emethod, evaled_target, evaled_args, env)?;
                    self.eval_single_expr_recursive(&result_par, env)
                }
            },
            None => Err(InterpreterError::ReduceError(format!(
                "Unimplemented expression: {:?}",
                expr
            ))),
        }
    }

    #[cfg(test)]
    fn eval_single_expr_recursive(&self, p: &Par, env: &Env<Par>) -> Result<Expr, InterpreterError> {
        if !p.sends.is_empty()
            || !p.receives.is_empty()
            || !p.news.is_empty()
            || !p.matches.is_empty()
            || !p.unforgeables.is_empty()
            || !p.bundles.is_empty()
        {
            Err(InterpreterError::ReduceError(String::from(
                "Error: parallel or non expression found where expression expected.",
            )))
        } else {
            match p.exprs.as_slice() {
                [e] => Ok(self.eval_expr_to_expr_recursive(e, env)?),
                _ => Err(InterpreterError::ReduceError(
                    "Error: Multiple expressions given.".to_string(),
                )),
            }
        }
    }

    #[cfg(test)]
    fn eval_to_i64_recursive(&self, p: &Par, env: &Env<Par>) -> Result<i64, InterpreterError> {
        if !p.sends.is_empty()
            && !p.receives.is_empty()
            && !p.news.is_empty()
            && !p.matches.is_empty()
            && !p.unforgeables.is_empty()
            && !p.bundles.is_empty()
        {
            Err(InterpreterError::ReduceError(String::from(
                "Error: parallel or non expression found where expression expected.",
            )))
        } else {
            match p.exprs.as_slice() {
                [Expr { expr_instance: Some(ExprInstance::GInt(v)) }] => Ok(*v),
                [Expr { expr_instance: Some(ExprInstance::EVarBody(EVar { v })) }] => {
                    let p = self.eval_var(&unwrap_option_safe(v.clone())?, env)?;
                    self.eval_to_i64_recursive(&p, env)
                }
                [e] => {
                    let evaled = self.eval_expr_to_expr_recursive(e, env)?;
                    Self::extract_i64(evaled)
                }
                _ => Err(InterpreterError::ReduceError(
                    "Error: Integer expected, or unimplemented expression.".to_string(),
                )),
            }
        }
    }

    #[cfg(test)]
    fn eval_to_bool_recursive(&self, p: &Par, env: &Env<Par>) -> Result<bool, InterpreterError> {
        if !p.sends.is_empty()
            && !p.receives.is_empty()
            && !p.news.is_empty()
            && !p.matches.is_empty()
            && !p.unforgeables.is_empty()
            && !p.bundles.is_empty()
        {
            Err(InterpreterError::ReduceError(String::from(
                "Error: parallel or non expression found where expression expected.",
            )))
        } else {
            match p.exprs.as_slice() {
                [Expr { expr_instance: Some(ExprInstance::GBool(b)) }] => Ok(*b),
                [Expr { expr_instance: Some(ExprInstance::EVarBody(EVar { v })) }] => {
                    let p = self.eval_var(&unwrap_option_safe(v.clone())?, env)?;
                    self.eval_to_bool_recursive(&p, env)
                }
                [e] => {
                    let evaled = self.eval_expr_to_expr_recursive(e, env)?;
                    Self::extract_bool(evaled)
                }
                _ => Err(InterpreterError::ReduceError(
                    "Error: Multiple expressions given.".to_string(),
                )),
            }
        }
    }


    fn nth_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct NthMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> NthMethod<'a> {
            fn local_nth(&self, ps: &[Par], nth: usize) -> Result<Par, InterpreterError> {
                if ps.len() > nth {
                    Ok(ps[nth].clone())
                } else {
                    Err(InterpreterError::ReduceError(format!(
                        "Error: index out of bound: {}",
                        nth
                    )))
                }
            }
        }

        impl<'a> Method for NthMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if args.len() != 1 {
                    return Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: "nth".to_string(),
                        expected: 1,
                        actual: args.len(),
                    });
                }

                self.outer
                    .metering
                    .reserve_primitive(nth_method_call_cost())?;
                let nth = self.outer.eval_to_i64(&args[0], env)? as usize;
                let v = self.outer.eval_single_expr(&p, env)?;

                match v.expr_instance.unwrap() {
                    ExprInstance::EListBody(EList { ps, .. }) => self.local_nth(&ps, nth),
                    ExprInstance::ETupleBody(ETuple { ps, .. }) => self.local_nth(&ps, nth),
                    ExprInstance::GByteArray(bs) => {
                        if nth < bs.len() {
                            let b = bs[nth]; // Convert to unsigned;
                            let p = new_gint_par(b as i64, Vec::new(), false);
                            Ok(p)
                        } else {
                            Err(InterpreterError::ReduceError(format!(
                                "Error: index out of bound: {}",
                                nth
                            )))
                        }
                    }
                    _ => Err(InterpreterError::ReduceError(String::from(
                        "Error: nth applied to something that wasn't a list or tuple.",
                    ))),
                }
            }
        }

        Box::new(NthMethod { outer: self })
    }

    fn to_byte_array_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct ToByteArrayMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> ToByteArrayMethod<'a> {
            fn serialize(&self, p: &Par) -> Result<Vec<u8>, InterpreterError> {
                Ok(p.encode_to_vec())
            }
        }

        impl<'a> Method for ToByteArrayMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if !args.is_empty() {
                    return Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: "toByteArray".to_string(),
                        expected: 0,
                        actual: args.len(),
                    });
                }

                let expr_evaled = self.outer.eval_expr(&p, env)?;
                let expr_subst =
                    self.outer
                        .substitute
                        .substitute_and_charge(&expr_evaled, 0, env)?;

                self.outer
                    .metering
                    .reserve_incremental_primitive(to_byte_array_cost(&expr_subst))?;
                let ba = self.serialize(&expr_subst)?;

                Ok(Par::default().with_exprs(vec![Expr {
                    expr_instance: Some(ExprInstance::GByteArray(ba)),
                }]))
            }
        }

        Box::new(ToByteArrayMethod { outer: self })
    }

    fn hex_to_bytes_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct HexToBytesMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> Method for HexToBytesMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                _env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if !args.is_empty() {
                    Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("hexToBytes"),
                        expected: 0,
                        actual: args.len(),
                    })
                } else {
                    match single_expr(&p) {
                        Some(expr) => match unwrap_option_safe(expr.expr_instance)? {
                            ExprInstance::GString(encoded) => {
                                self.outer
                                    .metering
                                    .reserve_incremental_primitive(hex_to_bytes_cost(&encoded))?;
                                Ok(Par::default().with_exprs(vec![Expr {
                                    expr_instance: Some(ExprInstance::GByteArray(
                                        StringOps::unsafe_decode_hex(encoded),
                                    )),
                                }]))
                            }

                            other => Err(InterpreterError::MethodNotDefined {
                                method: String::from("hexToBytes"),
                                other_type: get_type(other),
                            }),
                        },

                        None => Err(InterpreterError::ReduceError(String::from(
                            "Error: Method can only be called on singular expressions.",
                        ))),
                    }
                }
            }
        }

        Box::new(HexToBytesMethod { outer: self })
    }

    fn bytes_to_hex_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct BytesToHexMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> Method for BytesToHexMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                _env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if !args.is_empty() {
                    Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("bytesToHex"),
                        expected: 0,
                        actual: args.len(),
                    })
                } else {
                    match single_expr(&p) {
                        Some(expr) => match expr.expr_instance.unwrap() {
                            ExprInstance::GByteArray(bytes) => {
                                self.outer
                                    .metering
                                    .reserve_incremental_primitive(bytes_to_hex_cost(&bytes))?;

                                let str =
                                    bytes.iter().map(|byte| format!("{:02x}", byte)).collect();

                                Ok(new_gstring_par(str, Vec::new(), false))
                            }

                            other => Err(InterpreterError::MethodNotDefined {
                                method: String::from("BytesToHex"),
                                other_type: get_type(other),
                            }),
                        },

                        None => Err(InterpreterError::ReduceError(String::from(
                            "Error: Method can only be called on singular expressions.",
                        ))),
                    }
                }
            }
        }

        Box::new(BytesToHexMethod { outer: self })
    }

    fn to_utf8_bytes_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct ToUtf8BytesMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> Method for ToUtf8BytesMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                _env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if !args.is_empty() {
                    Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("toUtf8Bytes"),
                        expected: 0,
                        actual: args.len(),
                    })
                } else {
                    match single_expr(&p) {
                        Some(expr) => match expr.expr_instance.unwrap() {
                            ExprInstance::GString(utf8_string) => {
                                self.outer.metering.reserve_incremental_primitive(
                                    hex_to_bytes_cost(&utf8_string),
                                )?;

                                Ok(Par::default().with_exprs(vec![Expr {
                                    expr_instance: Some(ExprInstance::GByteArray(
                                        utf8_string.as_bytes().to_vec(),
                                    )),
                                }]))
                            }

                            other => Err(InterpreterError::MethodNotDefined {
                                method: String::from("toUtf8Bytes"),
                                other_type: get_type(other),
                            }),
                        },

                        None => Err(InterpreterError::ReduceError(String::from(
                            "Error: Method can only be called on singular expressions.",
                        ))),
                    }
                }
            }
        }

        Box::new(ToUtf8BytesMethod { outer: self })
    }

    fn union_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct UnionMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> UnionMethod<'a> {
            fn union(&self, base_expr: &Expr, other_expr: &Expr) -> Result<Expr, InterpreterError> {
                match (
                    base_expr.expr_instance.clone().unwrap(),
                    other_expr.expr_instance.clone().unwrap(),
                ) {
                    (ExprInstance::ESetBody(base_set), ExprInstance::ESetBody(other_set)) => {
                        let base_par_set = ParSetTypeMapper::eset_to_par_set(base_set);
                        let other_par_set = ParSetTypeMapper::eset_to_par_set(other_set);

                        let base_ps = base_par_set.ps;
                        let other_ps = other_par_set.ps;

                        self.outer
                            .metering
                            .reserve_incremental_primitive(union_cost(other_ps.length() as i64))?;

                        Ok(Expr {
                            expr_instance: Some(ExprInstance::ESetBody(
                                ParSetTypeMapper::par_set_to_eset(ParSet {
                                    ps: base_ps.union(other_ps.ps),
                                    connective_used: base_par_set.connective_used
                                        || other_par_set.connective_used,
                                    locally_free: union(
                                        base_par_set.locally_free,
                                        other_par_set.locally_free,
                                    ),
                                    remainder: None,
                                }),
                            )),
                        })
                    }

                    (ExprInstance::EMapBody(base_map), ExprInstance::EMapBody(other_map)) => {
                        let base_par_map = ParMapTypeMapper::emap_to_par_map(base_map);
                        let other_par_map = ParMapTypeMapper::emap_to_par_map(other_map.clone());

                        let mut base_sorted_par_map = base_par_map.ps;
                        let other_sorted_par_map = other_par_map.ps;

                        self.outer
                            .metering
                            .reserve_incremental_primitive(
                                union_cost(other_map.kvs.len() as i64),
                            )?;

                        Ok(Expr {
                            expr_instance: Some(ExprInstance::EMapBody(
                                ParMapTypeMapper::par_map_to_emap(ParMap::new(
                                    base_sorted_par_map
                                        .extend(other_sorted_par_map.into_iter().collect())
                                        .into_iter()
                                        .collect(),
                                    base_par_map.connective_used || other_par_map.connective_used,
                                    union(base_par_map.locally_free, other_par_map.locally_free),
                                    None,
                                )),
                            )),
                        })
                    }

                    (
                        ExprInstance::EPathmapBody(base_pathmap),
                        ExprInstance::EPathmapBody(other_pathmap),
                    ) => {
                        let base_rmap =
                            PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&base_pathmap);
                        let other_rmap =
                            PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&other_pathmap);

                        self.outer
                            .metering
                            .reserve_incremental_primitive(union_cost(
                                other_pathmap.ps.len() as i64
                            ))?;
                        let result_map = base_rmap.map.join(&other_rmap.map);

                        Ok(Expr {
                            expr_instance: Some(ExprInstance::EPathmapBody(
                                PathMapCrateTypeMapper::rholang_pathmap_to_e_pathmap(
                                    &result_map,
                                    base_rmap.connective_used || other_rmap.connective_used,
                                    &union(base_rmap.locally_free, other_rmap.locally_free),
                                    None,
                                ),
                            )),
                        })
                    }

                    (other, _) => Err(InterpreterError::MethodNotDefined {
                        method: String::from("union"),
                        other_type: get_type(other),
                    }),
                }
            }
        }

        impl<'a> Method for UnionMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if args.len() != 1 {
                    Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("union"),
                        expected: 1,
                        actual: args.len(),
                    })
                } else {
                    let base_expr = self.outer.eval_single_expr(&p, env)?;
                    let other_expr = self.outer.eval_single_expr(&args[0], env)?;
                    let result = self.union(&base_expr, &other_expr)?;
                    Ok(Par::default().with_exprs(vec![result]))
                }
            }
        }

        Box::new(UnionMethod { outer: self })
    }

    fn diff_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct DiffMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> DiffMethod<'a> {
            fn diff(&self, base_expr: &Expr, other_expr: &Expr) -> Result<Expr, InterpreterError> {
                match (
                    base_expr.expr_instance.clone().unwrap(),
                    other_expr.expr_instance.clone().unwrap(),
                ) {
                    (ExprInstance::ESetBody(base_set), ExprInstance::ESetBody(other_set)) => {
                        let base_par_set = ParSetTypeMapper::eset_to_par_set(base_set);
                        let other_par_set = ParSetTypeMapper::eset_to_par_set(other_set);

                        let base_ps = base_par_set.ps;
                        let other_ps = other_par_set.ps;

                        // diff is implemented in terms of foldLeft that at each step
                        // removes one element from the collection.
                        self.outer
                            .metering
                            .reserve_incremental_primitive(diff_cost(other_ps.length() as i64))?;

                        let base_sorted_pars_set: HashSet<Par> =
                            base_ps.sorted_pars.into_iter().collect();
                        let other_sorted_pars_set: HashSet<Par> =
                            other_ps.sorted_pars.into_iter().collect();
                        let new_par_set = ParSet::create_from_vec(
                            base_sorted_pars_set
                                .difference(&other_sorted_pars_set)
                                .into_iter()
                                .cloned()
                                .collect(),
                        );

                        Ok(Expr {
                            expr_instance: Some(ExprInstance::ESetBody(
                                ParSetTypeMapper::par_set_to_eset(new_par_set),
                            )),
                        })
                    }

                    (ExprInstance::EMapBody(base_emap), ExprInstance::EMapBody(other_emap)) => {
                        let base_par_map = ParMapTypeMapper::emap_to_par_map(base_emap);
                        let other_par_map = ParMapTypeMapper::emap_to_par_map(other_emap);

                        let mut base_ps = base_par_map.ps;
                        let other_ps = other_par_map.ps;

                        self.outer
                            .metering
                            .reserve_incremental_primitive(diff_cost(other_ps.length() as i64))?;

                        let new_par_map = ParMap::create_from_sorted_par_map(
                            base_ps.remove_multiple(other_ps.keys()),
                        );

                        Ok(Expr {
                            expr_instance: Some(ExprInstance::EMapBody(
                                ParMapTypeMapper::par_map_to_emap(new_par_map),
                            )),
                        })
                    }

                    (
                        ExprInstance::EPathmapBody(base_pathmap),
                        ExprInstance::EPathmapBody(other_pathmap),
                    ) => {
                        let base_rmap =
                            PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&base_pathmap);
                        let other_rmap =
                            PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&other_pathmap);

                        self.outer
                            .metering
                            .reserve_incremental_primitive(diff_cost(
                                other_pathmap.ps.len() as i64
                            ))?;
                        let result_map = base_rmap.map.subtract(&other_rmap.map);

                        Ok(Expr {
                            expr_instance: Some(ExprInstance::EPathmapBody(
                                PathMapCrateTypeMapper::rholang_pathmap_to_e_pathmap(
                                    &result_map,
                                    base_rmap.connective_used,
                                    &base_rmap.locally_free,
                                    None,
                                ),
                            )),
                        })
                    }

                    (other, _) => Err(InterpreterError::MethodNotDefined {
                        method: String::from("diff"),
                        other_type: get_type(other),
                    }),
                }
            }
        }

        impl<'a> Method for DiffMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if args.len() != 1 {
                    Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("diff"),
                        expected: 1,
                        actual: args.len(),
                    })
                } else {
                    let base_expr = self.outer.eval_single_expr(&p, env)?;
                    let other_expr = self.outer.eval_single_expr(&args[0], env)?;
                    let result = self.diff(&base_expr, &other_expr)?;
                    Ok(Par::default().with_exprs(vec![result]))
                }
            }
        }

        Box::new(DiffMethod { outer: self })
    }

    fn intersection_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct IntersectionMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> IntersectionMethod<'a> {
            fn intersection(
                &self,
                base_expr: &Expr,
                other_expr: &Expr,
            ) -> Result<Expr, InterpreterError> {
                match (
                    base_expr.expr_instance.clone().unwrap(),
                    other_expr.expr_instance.clone().unwrap(),
                ) {
                    (
                        ExprInstance::EPathmapBody(base_pathmap),
                        ExprInstance::EPathmapBody(other_pathmap),
                    ) => {
                        let base_rmap =
                            PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&base_pathmap);
                        let other_rmap =
                            PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&other_pathmap);

                        self.outer
                            .metering
                            .reserve_incremental_primitive(union_cost(
                                other_pathmap.ps.len() as i64
                            ))?;
                        let result_map = base_rmap.map.meet(&other_rmap.map);

                        Ok(Expr {
                            expr_instance: Some(ExprInstance::EPathmapBody(
                                PathMapCrateTypeMapper::rholang_pathmap_to_e_pathmap(
                                    &result_map,
                                    base_rmap.connective_used || other_rmap.connective_used,
                                    &union(base_rmap.locally_free, other_rmap.locally_free),
                                    None,
                                ),
                            )),
                        })
                    }

                    (other, _) => Err(InterpreterError::MethodNotDefined {
                        method: String::from("intersection"),
                        other_type: get_type(other),
                    }),
                }
            }
        }

        impl<'a> Method for IntersectionMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if args.len() != 1 {
                    Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("intersection"),
                        expected: 1,
                        actual: args.len(),
                    })
                } else {
                    let base_expr = self.outer.eval_single_expr(&p, env)?;
                    let other_par = &args[0];
                    let other_expr = self.outer.eval_single_expr(other_par, env)?;
                    let result = self.intersection(&base_expr, &other_expr)?;
                    Ok(Par::default().with_exprs(vec![result]))
                }
            }
        }

        Box::new(IntersectionMethod { outer: self })
    }

    fn restriction_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct RestrictionMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> RestrictionMethod<'a> {
            fn restriction(
                &self,
                base_expr: &Expr,
                other_expr: &Expr,
            ) -> Result<Expr, InterpreterError> {
                match (
                    base_expr.expr_instance.clone().unwrap(),
                    other_expr.expr_instance.clone().unwrap(),
                ) {
                    (
                        ExprInstance::EPathmapBody(base_pathmap),
                        ExprInstance::EPathmapBody(other_pathmap),
                    ) => {
                        let base_rmap =
                            PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&base_pathmap);
                        // W2b-1 SWEEP fix: `restriction` wraps PathMap::restrict, a
                        // PREFIX/subtrie op (base paths are kept under the paths
                        // LEADING TO VALUES in `other`). Under the codec a prefix
                        // is the NON-terminated segment concatenation — the same
                        // prefix-op principle applied to getSubtrie/pathExists/
                        // prunePath. Building `other` with terminated FULL keys
                        // (0x00) makes them prefix-free of base's keys, silently
                        // degenerating restrict to exact-match (a consensus-behavior
                        // change); non-terminated keys preserve the pre-codec
                        // prefix-restriction. `other_rmap` metadata is unused here
                        // (the result carries base's connective/locally_free).
                        let mut other_prefix_map =
                            models::rust::pathmap_integration::RholangPathMap::new();
                        for entry in &other_pathmap.ps {
                            other_prefix_map.insert(
                                segments_to_key(
                                    &models::rust::pathmap_integration::par_to_path(entry),
                                    false,
                                ),
                                entry.clone(),
                            );
                        }

                        self.outer
                            .metering
                            .reserve_incremental_primitive(union_cost(
                                other_pathmap.ps.len() as i64
                            ))?;
                        let result_map = base_rmap.map.restrict(&other_prefix_map);

                        Ok(Expr {
                            expr_instance: Some(ExprInstance::EPathmapBody(
                                PathMapCrateTypeMapper::rholang_pathmap_to_e_pathmap(
                                    &result_map,
                                    base_rmap.connective_used,
                                    &base_rmap.locally_free,
                                    None,
                                ),
                            )),
                        })
                    }

                    (other, _) => Err(InterpreterError::MethodNotDefined {
                        method: String::from("restriction"),
                        other_type: get_type(other),
                    }),
                }
            }
        }

        impl<'a> Method for RestrictionMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if args.len() != 1 {
                    Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("restriction"),
                        expected: 1,
                        actual: args.len(),
                    })
                } else {
                    let base_expr = self.outer.eval_single_expr(&p, env)?;
                    let other_par = &args[0];
                    let other_expr = self.outer.eval_single_expr(other_par, env)?;
                    let result = self.restriction(&base_expr, &other_expr)?;
                    Ok(Par::default().with_exprs(vec![result]))
                }
            }
        }

        Box::new(RestrictionMethod { outer: self })
    }

    fn drop_head_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct DropHeadMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> DropHeadMethod<'a> {
            fn drop_head(&self, base_expr: &Expr, n: i64) -> Result<Expr, InterpreterError> {
                match base_expr.expr_instance.clone().unwrap() {
                    ExprInstance::EPathmapBody(base_pathmap) => {
                        let base_rmap =
                            PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&base_pathmap);
                        if n < 0 {
                            return Err(InterpreterError::ReduceError(format!(
                                "dropHead argument must be non-negative, got: {}",
                                n
                            )));
                        }
                        self.outer
                            .metering
                            .reserve_incremental_primitive(union_cost(n))?;

                        // For dropHead, we need to return a new EPathMap with modified path elements
                        // Instead of using PathMap, directly construct the result elements
                        let mut result_elements = Vec::new();

                        for par in &base_pathmap.ps {
                            // Check if this Par is a list
                            if let Some(models::rhoapi::expr::ExprInstance::EListBody(list)) =
                                par.exprs.first().and_then(|e| e.expr_instance.as_ref())
                            {
                                // It's a list - drop n elements from the beginning
                                if list.ps.len() > n as usize {
                                    let remaining = list.ps[(n as usize)..].to_vec();
                                    let new_list = models::rhoapi::EList {
                                        ps: remaining,
                                        locally_free: list.locally_free.clone(),
                                        connective_used: list.connective_used,
                                        remainder: list.remainder.clone(),
                                    };
                                    let new_par = Par {
                                        exprs: vec![models::rhoapi::Expr {
                                            expr_instance: Some(
                                                models::rhoapi::expr::ExprInstance::EListBody(
                                                    new_list,
                                                ),
                                            ),
                                        }],
                                        ..par.clone()
                                    };
                                    result_elements.push(new_par);
                                }
                                // If not enough elements, skip this entry
                            } else {
                                // Not a list - can't drop head, skip or keep as-is based on n
                                if n == 0 {
                                    result_elements.push(par.clone());
                                }
                                // If n > 0, we skip non-list entries
                            }
                        }
                        Ok(Expr {
                            // EPathMap fix P3 (PM-2): constructor instead of
                            // a struct literal (private shadow cell).
                            expr_instance: Some(ExprInstance::EPathmapBody(
                                models::rhoapi::EPathMap::new(
                                    result_elements,
                                    base_rmap.locally_free.clone(),
                                    base_rmap.connective_used,
                                    None,
                                ),
                            )),
                        })
                    }

                    other => Err(InterpreterError::MethodNotDefined {
                        method: String::from("dropHead"),
                        other_type: get_type(other),
                    }),
                }
            }
        }

        impl<'a> Method for DropHeadMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if args.len() != 1 {
                    Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("dropHead"),
                        expected: 1,
                        actual: args.len(),
                    })
                } else {
                    let base_expr = self.outer.eval_single_expr(&p, env)?;
                    let n_par = &args[0];
                    let n = self.outer.eval_to_i64(n_par, env)?;
                    let result = self.drop_head(&base_expr, n)?;
                    Ok(Par::default().with_exprs(vec![result]))
                }
            }
        }

        Box::new(DropHeadMethod { outer: self })
    }

    fn run_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct RunMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> RunMethod<'a> {
            fn run(&self, base_expr: &Expr, _other_expr: &Expr) -> Result<Expr, InterpreterError> {
                match base_expr.expr_instance.clone().unwrap() {
                    ExprInstance::EPathmapBody(base_pathmap) => {
                        // For run method, we ignore the other parameter and return self
                        self.outer
                            .metering
                            .reserve_incremental_primitive(union_cost(1))?;

                        // Simply return the base PathMap unchanged
                        Ok(Expr {
                            expr_instance: Some(ExprInstance::EPathmapBody(base_pathmap)),
                        })
                    }

                    other => Err(InterpreterError::MethodNotDefined {
                        method: String::from("run"),
                        other_type: get_type(other),
                    }),
                }
            }
        }

        impl<'a> Method for RunMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if args.len() != 1 {
                    Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("run"),
                        expected: 1,
                        actual: args.len(),
                    })
                } else {
                    let base_expr = self.outer.eval_single_expr(&p, env)?;
                    let other_expr = self.outer.eval_single_expr(&args[0], env)?;
                    let result = self.run(&base_expr, &other_expr)?;
                    Ok(Par::default().with_exprs(vec![result]))
                }
            }
        }

        Box::new(RunMethod { outer: self })
    }

    // ============ ZIPPER METHODS ============

    fn read_zipper_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct ReadZipperMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> ReadZipperMethod<'a> {
            fn create_read_zipper(&self, base_expr: &Expr) -> Result<Expr, InterpreterError> {
                match base_expr.expr_instance.clone().unwrap() {
                    ExprInstance::EPathmapBody(pathmap) => {
                        // Create an EZipper from the PathMap
                        let ezipper = EZipper {
                            pathmap: Some(pathmap),
                            current_path: vec![], // Start at root
                            is_write_zipper: false,
                            locally_free: vec![],
                            connective_used: false,
                        };
                        Ok(Expr {
                            expr_instance: Some(ExprInstance::EZipperBody(ezipper)),
                        })
                    }
                    other => Err(InterpreterError::MethodNotDefined {
                        method: String::from("readZipper"),
                        other_type: get_type(other),
                    }),
                }
            }
        }

        impl<'a> Method for ReadZipperMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if !args.is_empty() {
                    return Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("readZipper"),
                        expected: 0,
                        actual: args.len(),
                    });
                }
                let base_expr = self.outer.eval_single_expr(&p, env)?;
                self.outer
                    .metering
                    .reserve_incremental_primitive(union_cost(1))?;
                let result = self.create_read_zipper(&base_expr)?;
                Ok(Par::default().with_exprs(vec![result]))
            }
        }

        Box::new(ReadZipperMethod { outer: self })
    }

    fn read_zipper_at_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct ReadZipperAtMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> ReadZipperAtMethod<'a> {
            fn create_read_zipper_at(
                &self,
                base_expr: &Expr,
                path_par: &Par,
            ) -> Result<Expr, InterpreterError> {
                match base_expr.expr_instance.clone().unwrap() {
                    ExprInstance::EPathmapBody(pathmap) => {
                        use models::rust::pathmap_integration::par_to_path;

                        // Convert the path argument to byte segments
                        let path_segments = par_to_path(path_par);

                        // Store the COMPLETE ORIGINAL PathMap for correct operations
                        // Display will show absolute paths, but operations will work correctly
                        // TODO: To show relative paths in display, we'd need to modify serialization/display code
                        //
                        // The map is MOVED into the zipper (the previous code deep-cloned the
                        // entire EPathMap a second time here — O(map) uncharged host work per
                        // readZipperAt call — after `expr_instance.clone()` already produced an
                        // owned copy). Field values are read out first; the resulting EZipper is
                        // byte-identical to what the clone-based construction produced.
                        let locally_free = pathmap.locally_free.clone();
                        let connective_used = pathmap.connective_used;

                        // Create an EZipper with the complete PathMap
                        // current_path indicates the position within the complete tree
                        let ezipper = EZipper {
                            pathmap: Some(pathmap),
                            current_path: path_segments,
                            is_write_zipper: false,
                            locally_free,
                            connective_used,
                        };

                        Ok(Expr {
                            expr_instance: Some(ExprInstance::EZipperBody(ezipper)),
                        })
                    }
                    other => Err(InterpreterError::MethodNotDefined {
                        method: String::from("readZipperAt"),
                        other_type: get_type(other),
                    }),
                }
            }
        }

        impl<'a> Method for ReadZipperAtMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if args.len() != 1 {
                    return Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("readZipperAt"),
                        expected: 1,
                        actual: args.len(),
                    });
                }
                let base_expr = self.outer.eval_single_expr(&p, env)?;
                let path = self.outer.eval_expr(&args[0], env)?;
                self.outer
                    .metering
                    .reserve_incremental_primitive(union_cost(1))?;
                let result = self.create_read_zipper_at(&base_expr, &path)?;
                Ok(Par::default().with_exprs(vec![result]))
            }
        }

        Box::new(ReadZipperAtMethod { outer: self })
    }

    fn write_zipper_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct WriteZipperMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> WriteZipperMethod<'a> {
            fn create_write_zipper(&self, base_expr: &Expr) -> Result<Expr, InterpreterError> {
                match base_expr.expr_instance.clone().unwrap() {
                    ExprInstance::EPathmapBody(pathmap) => {
                        // Create an EZipper for writing
                        let ezipper = EZipper {
                            pathmap: Some(pathmap),
                            current_path: vec![], // Start at root
                            is_write_zipper: true,
                            locally_free: vec![],
                            connective_used: false,
                        };
                        Ok(Expr {
                            expr_instance: Some(ExprInstance::EZipperBody(ezipper)),
                        })
                    }
                    other => Err(InterpreterError::MethodNotDefined {
                        method: String::from("writeZipper"),
                        other_type: get_type(other),
                    }),
                }
            }
        }

        impl<'a> Method for WriteZipperMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if !args.is_empty() {
                    return Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("writeZipper"),
                        expected: 0,
                        actual: args.len(),
                    });
                }
                let base_expr = self.outer.eval_single_expr(&p, env)?;
                self.outer
                    .metering
                    .reserve_incremental_primitive(union_cost(1))?;
                let result = self.create_write_zipper(&base_expr)?;
                Ok(Par::default().with_exprs(vec![result]))
            }
        }

        Box::new(WriteZipperMethod { outer: self })
    }

    fn write_zipper_at_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct WriteZipperAtMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> WriteZipperAtMethod<'a> {
            fn create_write_zipper_at(
                &self,
                base_expr: &Expr,
                path_par: &Par,
            ) -> Result<Expr, InterpreterError> {
                match base_expr.expr_instance.clone().unwrap() {
                    ExprInstance::EPathmapBody(pathmap) => {
                        use models::rust::pathmap_integration::par_to_path;

                        // Convert the path argument to byte segments
                        let path_segments = par_to_path(path_par);

                        // Store the COMPLETE ORIGINAL PathMap for correct operations
                        let complete_pathmap = pathmap.clone();

                        // Create an EZipper with the complete PathMap (write mode)
                        let ezipper = EZipper {
                            pathmap: Some(complete_pathmap),
                            current_path: path_segments.clone(),
                            is_write_zipper: true,
                            locally_free: pathmap.locally_free.clone(),
                            connective_used: pathmap.connective_used,
                        };

                        Ok(Expr {
                            expr_instance: Some(ExprInstance::EZipperBody(ezipper)),
                        })
                    }
                    other => Err(InterpreterError::MethodNotDefined {
                        method: String::from("writeZipperAt"),
                        other_type: get_type(other),
                    }),
                }
            }
        }

        impl<'a> Method for WriteZipperAtMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if args.len() != 1 {
                    return Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("writeZipperAt"),
                        expected: 1,
                        actual: args.len(),
                    });
                }
                let base_expr = self.outer.eval_single_expr(&p, env)?;
                let path = self.outer.eval_expr(&args[0], env)?;
                self.outer
                    .metering
                    .reserve_incremental_primitive(union_cost(1))?;
                let result = self.create_write_zipper_at(&base_expr, &path)?;
                Ok(Par::default().with_exprs(vec![result]))
            }
        }

        Box::new(WriteZipperAtMethod { outer: self })
    }

    fn descend_to_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct DescendToMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> DescendToMethod<'a> {
            fn descend_to(
                &self,
                base_expr: &Expr,
                path_par: &Par,
            ) -> Result<Expr, InterpreterError> {
                match base_expr.expr_instance.clone().unwrap() {
                    ExprInstance::EZipperBody(mut zipper) => {
                        use models::rust::pathmap_integration::par_to_path;

                        // Convert the path argument to byte segments
                        let path_segments = par_to_path(path_par);

                        // Update the zipper's current_path to navigate to the new location
                        // Append the new path segments to the current path
                        zipper.current_path.extend(path_segments);

                        Ok(Expr {
                            expr_instance: Some(ExprInstance::EZipperBody(zipper)),
                        })
                    }
                    other => Err(InterpreterError::MethodNotDefined {
                        method: String::from("descendTo"),
                        other_type: get_type(other),
                    }),
                }
            }
        }

        impl<'a> Method for DescendToMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if args.len() != 1 {
                    return Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("descendTo"),
                        expected: 1,
                        actual: args.len(),
                    });
                }
                let base_expr = self.outer.eval_single_expr(&p, env)?;
                let path = self.outer.eval_expr(&args[0], env)?;
                self.outer
                    .metering
                    .reserve_incremental_primitive(union_cost(1))?;
                let result = self.descend_to(&base_expr, &path)?;
                Ok(Par::default().with_exprs(vec![result]))
            }
        }

        Box::new(DescendToMethod { outer: self })
    }

    fn get_leaf_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct GetLeafMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> GetLeafMethod<'a> {
            fn get_leaf(&self, base_expr: &Expr) -> Result<Par, InterpreterError> {
                match base_expr.expr_instance.clone().unwrap() {
                    ExprInstance::EZipperBody(zipper) => {
                        // Get the pathmap from the zipper
                        let pathmap = zipper.pathmap.as_ref().expect("zipper pathmap was None");
                        let pathmap_result =
                            PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(pathmap);
                        let rholang_pathmap = pathmap_result.map;

                        // Use the zipper's current_path to look up the value
                        // Build the key from current_path segments (same encoding as create_pathmap_from_elements)
                        let key: Vec<u8> = segments_to_key(&zipper.current_path, true);

                        // Look up value at this path
                        if let Some(value) = rholang_pathmap.get(&key) {
                            Ok(value.clone())
                        } else {
                            Ok(Par::default()) // Nil - no value at this path
                        }
                    }
                    ExprInstance::EPathmapBody(pathmap) => {
                        // Convert EPathMap to RholangPathMap
                        let pathmap_result =
                            PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&pathmap);
                        let rholang_pathmap = pathmap_result.map;

                        // Create a read zipper and get the value at current position
                        let read_zipper = RholangReadZipper::new(
                            &rholang_pathmap,
                            pathmap_result.connective_used,
                            pathmap_result.locally_free,
                        );

                        // Get value at current position (root)
                        if let Some(value) = read_zipper.get_val() {
                            Ok(value.clone())
                        } else {
                            Ok(Par::default()) // Nil
                        }
                    }
                    other => Err(InterpreterError::MethodNotDefined {
                        method: String::from("getLeaf"),
                        other_type: get_type(other),
                    }),
                }
            }
        }

        impl<'a> Method for GetLeafMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if !args.is_empty() {
                    return Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("getLeaf"),
                        expected: 0,
                        actual: args.len(),
                    });
                }
                let base_expr = self.outer.eval_single_expr(&p, env)?;
                self.outer.metering.reserve_primitive(lookup_cost())?;
                self.get_leaf(&base_expr)
            }
        }

        Box::new(GetLeafMethod { outer: self })
    }

    fn get_subtrie_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct GetSubtrieMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> GetSubtrieMethod<'a> {
            fn get_subtrie(&self, base_expr: &Expr) -> Result<Par, InterpreterError> {
                match base_expr.expr_instance.clone().unwrap() {
                    ExprInstance::EZipperBody(zipper) => {
                        // Get the pathmap from the zipper
                        let pathmap = zipper.pathmap.as_ref().expect("zipper pathmap was None");
                        let pathmap_result =
                            PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(pathmap);
                        let rholang_pathmap = pathmap_result.map;

                        // Build prefix key from current_path
                        let prefix_key: Vec<u8> = segments_to_key(&zipper.current_path, false);

                        // Collect all entries with this prefix — native subtrie descent
                        // (O(prefix + subtrie) instead of the previous whole-map scan);
                        // yields the same values in the same trie-DFS order (prefix keys
                        // are a contiguous run of the byte-lex iteration order).
                        let subtrie_elements = collect_subtrie_values(&rholang_pathmap, &prefix_key);

                        // Return as PathMap
                        Ok(Par::default().with_exprs(vec![Expr {
                            // EPathMap fix P3 (PM-2): constructor instead of
                            // a struct literal (private shadow cell).
                            expr_instance: Some(ExprInstance::EPathmapBody(EPathMap::new(
                                subtrie_elements,
                                pathmap_result.locally_free,
                                pathmap_result.connective_used,
                                None,
                            ))),
                        }]))
                    }
                    ExprInstance::EPathmapBody(pathmap) => {
                        // For PathMap without zipper, return entire PathMap (all is subtrie at root)
                        Ok(Par::default().with_exprs(vec![Expr {
                            expr_instance: Some(ExprInstance::EPathmapBody(pathmap)),
                        }]))
                    }
                    other => Err(InterpreterError::MethodNotDefined {
                        method: String::from("getSubtrie"),
                        other_type: get_type(other),
                    }),
                }
            }
        }

        impl<'a> Method for GetSubtrieMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if !args.is_empty() {
                    return Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("getSubtrie"),
                        expected: 0,
                        actual: args.len(),
                    });
                }
                let base_expr = self.outer.eval_single_expr(&p, env)?;
                self.outer.metering.reserve_primitive(lookup_cost())?;
                self.get_subtrie(&base_expr)
            }
        }

        Box::new(GetSubtrieMethod { outer: self })
    }

    fn set_leaf_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct SetLeafMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> SetLeafMethod<'a> {
            fn set_leaf(&self, base_expr: &Expr, value: &Par) -> Result<Expr, InterpreterError> {
                match base_expr.expr_instance.clone().unwrap() {
                    ExprInstance::EZipperBody(zipper) => {
                        // For a write zipper, set value at current position
                        let mut pathmap = zipper.pathmap.expect("zipper pathmap was None");
                        // L2: sanctioned CoW mutation — resets any inherited
                        // intern cell and detaches the shared payload.
                        pathmap.ps_make_mut().push(value.clone());
                        // Return the modified PathMap (not zipper)
                        Ok(Expr {
                            expr_instance: Some(ExprInstance::EPathmapBody(pathmap)),
                        })
                    }
                    ExprInstance::EPathmapBody(mut pathmap) => {
                        // For a write zipper, set value at current position
                        // For now, add to the pathmap
                        // L2: sanctioned CoW mutation (cell reset + detach).
                        pathmap.ps_make_mut().push(value.clone());
                        Ok(Expr {
                            expr_instance: Some(ExprInstance::EPathmapBody(pathmap)),
                        })
                    }
                    other => Err(InterpreterError::MethodNotDefined {
                        method: String::from("setLeaf"),
                        other_type: get_type(other),
                    }),
                }
            }
        }

        impl<'a> Method for SetLeafMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if args.len() != 1 {
                    return Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("setLeaf"),
                        expected: 1,
                        actual: args.len(),
                    });
                }
                let base_expr = self.outer.eval_single_expr(&p, env)?;
                let value = self.outer.eval_expr(&args[0], env)?;
                self.outer.metering.reserve_primitive(add_cost())?;
                let result = self.set_leaf(&base_expr, &value)?;
                Ok(Par::default().with_exprs(vec![result]))
            }
        }

        Box::new(SetLeafMethod { outer: self })
    }

    fn set_subtrie_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct SetSubtrieMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> SetSubtrieMethod<'a> {
            fn set_subtrie(
                &self,
                base_expr: &Expr,
                source_par: &Par,
            ) -> Result<Expr, InterpreterError> {
                match (
                    base_expr.expr_instance.clone().unwrap(),
                    source_par
                        .exprs
                        .first()
                        .and_then(|e| e.expr_instance.clone()),
                ) {
                    // Only works on write zippers
                    (
                        ExprInstance::EZipperBody(zipper),
                        Some(ExprInstance::EPathmapBody(source)),
                    ) if zipper.is_write_zipper => {
                        // Step 1: Extract base PathMap and build prefix
                        let pathmap = zipper.pathmap.expect("zipper pathmap was None");
                        let pathmap_result =
                            PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&pathmap);
                        let mut rholang_pathmap = pathmap_result.map;

                        let prefix_key: Vec<u8> = segments_to_key(&zipper.current_path, false);

                        // Step 2: Remove all entries with this prefix
                        let keys_to_remove: Vec<Vec<u8>> = rholang_pathmap
                            .iter()
                            .filter_map(|(key, _)| {
                                if key.starts_with(&prefix_key) {
                                    Some(key.clone())
                                } else {
                                    None
                                }
                            })
                            .collect();

                        for key in keys_to_remove {
                            rholang_pathmap.remove(&key);
                        }

                        // Step 3: Add source entries with prepended prefix
                        for source_entry in source.ps.iter() {
                            use models::rust::pathmap_integration::par_to_path;
                            let source_segments = par_to_path(source_entry);

                            // Prepend current_path to make absolute
                            let mut absolute_segments = zipper.current_path.clone();
                            absolute_segments.extend(source_segments.clone());

                            // Encode as key
                            let key: Vec<u8> = segments_to_key(&absolute_segments, true);

                            // Build the Par that represents the absolute path
                            // Extract elements from an existing entry to understand their structure
                            let mut absolute_elements = Vec::new();

                            // Find an existing entry that starts with current_path
                            let found_existing = if let Some(existing_entry) =
                                pathmap.ps.iter().find(|entry| {
                                    if let Some(ExprInstance::EListBody(existing_list)) =
                                        &entry.exprs.first().and_then(|e| e.expr_instance.as_ref())
                                    {
                                        if existing_list.ps.len() < zipper.current_path.len() {
                                            return false;
                                        }
                                        // Check if the entry actually starts with current_path
                                        use models::rust::pathmap_integration::par_to_path;
                                        let entry_segments = par_to_path(entry);
                                        entry_segments.starts_with(&zipper.current_path)
                                    } else {
                                        false
                                    }
                                }) {
                                if let Some(ExprInstance::EListBody(existing_list)) =
                                    &existing_entry
                                        .exprs
                                        .first()
                                        .and_then(|e| e.expr_instance.as_ref())
                                {
                                    // Take first N elements where N = current_path length
                                    absolute_elements.extend(
                                        existing_list.ps[..zipper.current_path.len()].to_vec(),
                                    );
                                }
                                true
                            } else {
                                false
                            };

                            // If no existing entry found, reconstruct Par elements from current_path bytes
                            if !found_existing {
                                // W2b-1 (D2): reconstruct the current_path
                                // elements faithfully via the codec. Each
                                // segment is `encode_trie_segment(element)`, so
                                // the split-form path `concat(segments) ∥ 0x00`
                                // decodes back to the ground EList of those
                                // elements — restoring ANY eval_stable element
                                // (nested lists, numerics, GPrivate leaves), a
                                // behavior FIX over the former lossy
                                // GString-only SExpr quote-strip.
                                use models::rust::canonical_path::decode_trie_path;
                                let full_key =
                                    segments_to_key(&zipper.current_path, true);
                                if let Ok(decoded) = decode_trie_path(&full_key) {
                                    if let Some(ExprInstance::EListBody(list)) = decoded
                                        .exprs
                                        .first()
                                        .and_then(|e| e.expr_instance.as_ref())
                                    {
                                        absolute_elements.extend(list.ps.clone());
                                    }
                                }
                            }

                            // Add source_entry's elements
                            if let Some(ExprInstance::EListBody(source_list)) = &source_entry
                                .exprs
                                .first()
                                .and_then(|e| e.expr_instance.as_ref())
                            {
                                absolute_elements.extend(source_list.ps.clone());
                            }

                            // Create the absolute path Par
                            let absolute_path_par = Par::default().with_exprs(vec![Expr {
                                expr_instance: Some(ExprInstance::EListBody(
                                    models::rhoapi::EList {
                                        ps: absolute_elements,
                                        locally_free: vec![],
                                        connective_used: false,
                                        remainder: None,
                                    },
                                )),
                            }]);

                            rholang_pathmap.insert(key, absolute_path_par);
                        }

                        // Step 3b: If source is empty, add current_path as entry
                        if source.ps.is_empty() && !zipper.current_path.is_empty() {
                            // Encode current_path as key
                            let key: Vec<u8> = segments_to_key(&zipper.current_path, true);

                            // Build the Par for current_path
                            let mut absolute_elements = Vec::new();

                            // Find an existing entry that starts with current_path
                            let found_existing = if let Some(existing_entry) =
                                pathmap.ps.iter().find(|entry| {
                                    if let Some(ExprInstance::EListBody(existing_list)) =
                                        &entry.exprs.first().and_then(|e| e.expr_instance.as_ref())
                                    {
                                        if existing_list.ps.len() < zipper.current_path.len() {
                                            return false;
                                        }
                                        // Check if the entry actually starts with current_path
                                        use models::rust::pathmap_integration::par_to_path;
                                        let entry_segments = par_to_path(entry);
                                        entry_segments.starts_with(&zipper.current_path)
                                    } else {
                                        false
                                    }
                                }) {
                                if let Some(ExprInstance::EListBody(existing_list)) =
                                    &existing_entry
                                        .exprs
                                        .first()
                                        .and_then(|e| e.expr_instance.as_ref())
                                {
                                    // Take first N elements where N = current_path length
                                    absolute_elements.extend(
                                        existing_list.ps[..zipper.current_path.len()].to_vec(),
                                    );
                                }
                                true
                            } else {
                                false
                            };

                            // If no existing entry found, reconstruct Par elements from current_path bytes
                            if !found_existing {
                                // W2b-1 (D2): reconstruct the current_path
                                // elements faithfully via the codec. Each
                                // segment is `encode_trie_segment(element)`, so
                                // the split-form path `concat(segments) ∥ 0x00`
                                // decodes back to the ground EList of those
                                // elements — restoring ANY eval_stable element
                                // (nested lists, numerics, GPrivate leaves), a
                                // behavior FIX over the former lossy
                                // GString-only SExpr quote-strip.
                                use models::rust::canonical_path::decode_trie_path;
                                let full_key =
                                    segments_to_key(&zipper.current_path, true);
                                if let Ok(decoded) = decode_trie_path(&full_key) {
                                    if let Some(ExprInstance::EListBody(list)) = decoded
                                        .exprs
                                        .first()
                                        .and_then(|e| e.expr_instance.as_ref())
                                    {
                                        absolute_elements.extend(list.ps.clone());
                                    }
                                }
                            }

                            // Create the Par for current_path
                            let current_path_par = Par::default().with_exprs(vec![Expr {
                                expr_instance: Some(ExprInstance::EListBody(
                                    models::rhoapi::EList {
                                        ps: absolute_elements,
                                        locally_free: vec![],
                                        connective_used: false,
                                        remainder: None,
                                    },
                                )),
                            }]);

                            rholang_pathmap.insert(key, current_path_par);
                        }

                        // Step 4: Convert back to EPathMap
                        let result_pathmap = PathMapCrateTypeMapper::rholang_pathmap_to_e_pathmap(
                            &rholang_pathmap,
                            pathmap_result.connective_used,
                            &pathmap_result.locally_free,
                            None,
                        );

                        Ok(Expr {
                            expr_instance: Some(ExprInstance::EPathmapBody(result_pathmap)),
                        })
                    }

                    // Error cases
                    (ExprInstance::EZipperBody(zipper), _) if !zipper.is_write_zipper => {
                        Err(InterpreterError::MethodNotDefined {
                            method: String::from("setSubtrie (requires write zipper)"),
                            other_type: "read zipper".to_string(),
                        })
                    }
                    (other, _) => Err(InterpreterError::MethodNotDefined {
                        method: String::from("setSubtrie"),
                        other_type: get_type(other),
                    }),
                }
            }
        }

        impl<'a> Method for SetSubtrieMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if args.len() != 1 {
                    return Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("setSubtrie"),
                        expected: 1,
                        actual: args.len(),
                    });
                }
                let base_expr = self.outer.eval_single_expr(&p, env)?;
                let source_par = self.outer.eval_expr(&args[0], env)?;
                self.outer
                    .metering
                    .reserve_incremental_primitive(union_cost(1))?;
                let result = self.set_subtrie(&base_expr, &source_par)?;
                Ok(Par::default().with_exprs(vec![result]))
            }
        }

        Box::new(SetSubtrieMethod { outer: self })
    }

    fn remove_leaf_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct RemoveLeafMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> RemoveLeafMethod<'a> {
            fn remove_leaf(&self, base_expr: &Expr) -> Result<Expr, InterpreterError> {
                match base_expr.expr_instance.clone().unwrap() {
                    ExprInstance::EZipperBody(zipper) => {
                        // Extract pathmap from zipper
                        let pathmap = zipper.pathmap.expect("zipper pathmap was None");
                        let pathmap_result =
                            PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&pathmap);
                        let mut rholang_pathmap = pathmap_result.map;

                        // Build key from current_path
                        let key: Vec<u8> = segments_to_key(&zipper.current_path, true);

                        // Remove value at this path
                        rholang_pathmap.remove(&key);

                        // Convert back to EPathMap
                        let result_pathmap = PathMapCrateTypeMapper::rholang_pathmap_to_e_pathmap(
                            &rholang_pathmap,
                            pathmap_result.connective_used,
                            &pathmap_result.locally_free,
                            None,
                        );

                        Ok(Expr {
                            expr_instance: Some(ExprInstance::EPathmapBody(result_pathmap)),
                        })
                    }
                    ExprInstance::EPathmapBody(mut pathmap) => {
                        // Remove value at current position (root)
                        // L2: sanctioned CoW mutation (cell reset + detach).
                        pathmap.ps_make_mut().pop();
                        Ok(Expr {
                            expr_instance: Some(ExprInstance::EPathmapBody(pathmap)),
                        })
                    }
                    other => Err(InterpreterError::MethodNotDefined {
                        method: String::from("removeLeaf"),
                        other_type: get_type(other),
                    }),
                }
            }
        }

        impl<'a> Method for RemoveLeafMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if !args.is_empty() {
                    return Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("removeLeaf"),
                        expected: 0,
                        actual: args.len(),
                    });
                }
                let base_expr = self.outer.eval_single_expr(&p, env)?;
                self.outer.metering.reserve_primitive(remove_cost())?;
                let result = self.remove_leaf(&base_expr)?;
                Ok(Par::default().with_exprs(vec![result]))
            }
        }

        Box::new(RemoveLeafMethod { outer: self })
    }

    fn remove_branches_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct RemoveBranchesMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> RemoveBranchesMethod<'a> {
            fn remove_branches(&self, base_expr: &Expr) -> Result<Expr, InterpreterError> {
                match base_expr.expr_instance.clone().unwrap() {
                    ExprInstance::EZipperBody(zipper) => {
                        // Extract pathmap from zipper
                        let pathmap = zipper.pathmap.expect("zipper pathmap was None");
                        let pathmap_result =
                            PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&pathmap);
                        let mut rholang_pathmap = pathmap_result.map;

                        // Build prefix key from current_path
                        let prefix_key: Vec<u8> = segments_to_key(&zipper.current_path, false);

                        // Remove all branches with this prefix
                        // Collect keys to remove (can't modify while iterating)
                        let keys_to_remove: Vec<Vec<u8>> = rholang_pathmap
                            .iter()
                            .filter_map(|(key, _)| {
                                if key.starts_with(&prefix_key) && key.len() > prefix_key.len() {
                                    Some(key.clone())
                                } else {
                                    None
                                }
                            })
                            .collect();

                        // Remove the collected keys
                        for key in keys_to_remove {
                            rholang_pathmap.remove(&key);
                        }

                        // Convert back to EPathMap
                        let result_pathmap = PathMapCrateTypeMapper::rholang_pathmap_to_e_pathmap(
                            &rholang_pathmap,
                            pathmap_result.connective_used,
                            &pathmap_result.locally_free,
                            None,
                        );

                        Ok(Expr {
                            expr_instance: Some(ExprInstance::EPathmapBody(result_pathmap)),
                        })
                    }
                    ExprInstance::EPathmapBody(pathmap) => {
                        // Remove all branches below current position (root = remove everything)
                        Ok(Expr {
                            // EPathMap fix P3 (PM-2): constructor instead of
                            // a struct literal (private shadow cell).
                            expr_instance: Some(ExprInstance::EPathmapBody(
                                models::rhoapi::EPathMap::new(
                                    vec![],
                                    pathmap.locally_free,
                                    pathmap.connective_used,
                                    pathmap.remainder,
                                ),
                            )),
                        })
                    }
                    other => Err(InterpreterError::MethodNotDefined {
                        method: String::from("removeBranches"),
                        other_type: get_type(other),
                    }),
                }
            }
        }

        impl<'a> Method for RemoveBranchesMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if !args.is_empty() {
                    return Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("removeBranches"),
                        expected: 0,
                        actual: args.len(),
                    });
                }
                let base_expr = self.outer.eval_single_expr(&p, env)?;
                self.outer.metering.reserve_primitive(remove_cost())?;
                let result = self.remove_branches(&base_expr)?;
                Ok(Par::default().with_exprs(vec![result]))
            }
        }

        Box::new(RemoveBranchesMethod { outer: self })
    }

    fn graft_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct GraftMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> GraftMethod<'a> {
            fn graft(
                &self,
                base_expr: &Expr,
                source_expr: &Expr,
            ) -> Result<Expr, InterpreterError> {
                match (
                    base_expr.expr_instance.clone().unwrap(),
                    source_expr.expr_instance.clone().unwrap(),
                ) {
                    // Both are zippers
                    (
                        ExprInstance::EZipperBody(dest_zipper),
                        ExprInstance::EZipperBody(source_zipper),
                    ) => {
                        let mut dest_pathmap =
                            dest_zipper.pathmap.expect("dest zipper pathmap was None");
                        let source_pathmap = source_zipper
                            .pathmap
                            .expect("source zipper pathmap was None");

                        // Graft: copy subtrie from source to destination
                        // L2: sanctioned CoW mutation on the destination
                        // (cell reset + detach); the source payload is
                        // extracted by value (`into_vec` moves when unshared,
                        // clones when shared — the copy every pre-L2 clone of
                        // the source already paid up front).
                        dest_pathmap
                            .ps_make_mut()
                            .extend(source_pathmap.ps.into_vec());

                        Ok(Expr {
                            expr_instance: Some(ExprInstance::EPathmapBody(dest_pathmap)),
                        })
                    }
                    // Destination is zipper, source is PathMap
                    (
                        ExprInstance::EZipperBody(dest_zipper),
                        ExprInstance::EPathmapBody(source_pathmap),
                    ) => {
                        let mut dest_pathmap =
                            dest_zipper.pathmap.expect("dest zipper pathmap was None");

                        // Graft: copy subtrie from source to destination
                        // L2: sanctioned CoW mutation on the destination
                        // (cell reset + detach); the source payload is
                        // extracted by value (`into_vec` moves when unshared,
                        // clones when shared — the copy every pre-L2 clone of
                        // the source already paid up front).
                        dest_pathmap
                            .ps_make_mut()
                            .extend(source_pathmap.ps.into_vec());

                        Ok(Expr {
                            expr_instance: Some(ExprInstance::EPathmapBody(dest_pathmap)),
                        })
                    }
                    // Destination is PathMap, source is zipper
                    (
                        ExprInstance::EPathmapBody(mut dest_pathmap),
                        ExprInstance::EZipperBody(source_zipper),
                    ) => {
                        let source_pathmap = source_zipper
                            .pathmap
                            .expect("source zipper pathmap was None");

                        // Graft: copy subtrie from source to destination
                        // L2: sanctioned CoW mutation on the destination
                        // (cell reset + detach); the source payload is
                        // extracted by value (`into_vec` moves when unshared,
                        // clones when shared — the copy every pre-L2 clone of
                        // the source already paid up front).
                        dest_pathmap
                            .ps_make_mut()
                            .extend(source_pathmap.ps.into_vec());

                        Ok(Expr {
                            expr_instance: Some(ExprInstance::EPathmapBody(dest_pathmap)),
                        })
                    }
                    // Both are PathMaps (existing case)
                    (
                        ExprInstance::EPathmapBody(mut dest_pathmap),
                        ExprInstance::EPathmapBody(source_pathmap),
                    ) => {
                        // Graft: copy subtrie from source to destination
                        // L2: sanctioned CoW mutation on the destination
                        // (cell reset + detach); the source payload is
                        // extracted by value (`into_vec` moves when unshared,
                        // clones when shared — the copy every pre-L2 clone of
                        // the source already paid up front).
                        dest_pathmap
                            .ps_make_mut()
                            .extend(source_pathmap.ps.into_vec());
                        Ok(Expr {
                            expr_instance: Some(ExprInstance::EPathmapBody(dest_pathmap)),
                        })
                    }
                    (other, _) => Err(InterpreterError::MethodNotDefined {
                        method: String::from("graft"),
                        other_type: get_type(other),
                    }),
                }
            }
        }

        impl<'a> Method for GraftMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if args.len() != 1 {
                    return Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("graft"),
                        expected: 1,
                        actual: args.len(),
                    });
                }
                let base_expr = self.outer.eval_single_expr(&p, env)?;
                let source_expr = self.outer.eval_single_expr(&args[0], env)?;
                self.outer
                    .metering
                    .reserve_incremental_primitive(union_cost(1))?;
                let result = self.graft(&base_expr, &source_expr)?;
                Ok(Par::default().with_exprs(vec![result]))
            }
        }

        Box::new(GraftMethod { outer: self })
    }

    fn join_into_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct JoinIntoMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> JoinIntoMethod<'a> {
            fn join_into(
                &self,
                base_expr: &Expr,
                source_expr: &Expr,
            ) -> Result<Expr, InterpreterError> {
                match (
                    base_expr.expr_instance.clone().unwrap(),
                    source_expr.expr_instance.clone().unwrap(),
                ) {
                    // Both are zippers
                    (
                        ExprInstance::EZipperBody(base_zipper),
                        ExprInstance::EZipperBody(source_zipper),
                    ) => {
                        let base_pathmap =
                            base_zipper.pathmap.expect("base zipper pathmap was None");
                        let source_pathmap = source_zipper
                            .pathmap
                            .expect("source zipper pathmap was None");

                        let base_rmap =
                            PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&base_pathmap);
                        let source_rmap =
                            PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&source_pathmap);

                        self.outer
                            .metering
                            .reserve_incremental_primitive(union_cost(
                                source_pathmap.ps.len() as i64
                            ))?;
                        let result_map = base_rmap.map.join(&source_rmap.map);

                        Ok(Expr {
                            expr_instance: Some(ExprInstance::EPathmapBody(
                                PathMapCrateTypeMapper::rholang_pathmap_to_e_pathmap(
                                    &result_map,
                                    base_rmap.connective_used || source_rmap.connective_used,
                                    &union(base_rmap.locally_free, source_rmap.locally_free),
                                    None,
                                ),
                            )),
                        })
                    }
                    // Base is zipper, source is PathMap
                    (
                        ExprInstance::EZipperBody(base_zipper),
                        ExprInstance::EPathmapBody(source_pathmap),
                    ) => {
                        let base_pathmap =
                            base_zipper.pathmap.expect("base zipper pathmap was None");

                        let base_rmap =
                            PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&base_pathmap);
                        let source_rmap =
                            PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&source_pathmap);

                        self.outer
                            .metering
                            .reserve_incremental_primitive(union_cost(
                                source_pathmap.ps.len() as i64
                            ))?;
                        let result_map = base_rmap.map.join(&source_rmap.map);

                        Ok(Expr {
                            expr_instance: Some(ExprInstance::EPathmapBody(
                                PathMapCrateTypeMapper::rholang_pathmap_to_e_pathmap(
                                    &result_map,
                                    base_rmap.connective_used || source_rmap.connective_used,
                                    &union(base_rmap.locally_free, source_rmap.locally_free),
                                    None,
                                ),
                            )),
                        })
                    }
                    // Base is PathMap, source is zipper
                    (
                        ExprInstance::EPathmapBody(base_pathmap),
                        ExprInstance::EZipperBody(source_zipper),
                    ) => {
                        let source_pathmap = source_zipper
                            .pathmap
                            .expect("source zipper pathmap was None");

                        let base_rmap =
                            PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&base_pathmap);
                        let source_rmap =
                            PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&source_pathmap);

                        self.outer
                            .metering
                            .reserve_incremental_primitive(union_cost(
                                source_pathmap.ps.len() as i64
                            ))?;
                        let result_map = base_rmap.map.join(&source_rmap.map);

                        Ok(Expr {
                            expr_instance: Some(ExprInstance::EPathmapBody(
                                PathMapCrateTypeMapper::rholang_pathmap_to_e_pathmap(
                                    &result_map,
                                    base_rmap.connective_used || source_rmap.connective_used,
                                    &union(base_rmap.locally_free, source_rmap.locally_free),
                                    None,
                                ),
                            )),
                        })
                    }
                    // Both are PathMaps (existing case)
                    (
                        ExprInstance::EPathmapBody(base_pathmap),
                        ExprInstance::EPathmapBody(source_pathmap),
                    ) => {
                        // JoinInto: union-merge subtries
                        let base_rmap =
                            PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&base_pathmap);
                        let source_rmap =
                            PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&source_pathmap);

                        self.outer
                            .metering
                            .reserve_incremental_primitive(union_cost(
                                source_pathmap.ps.len() as i64
                            ))?;
                        let result_map = base_rmap.map.join(&source_rmap.map);

                        Ok(Expr {
                            expr_instance: Some(ExprInstance::EPathmapBody(
                                PathMapCrateTypeMapper::rholang_pathmap_to_e_pathmap(
                                    &result_map,
                                    base_rmap.connective_used || source_rmap.connective_used,
                                    &union(base_rmap.locally_free, source_rmap.locally_free),
                                    None,
                                ),
                            )),
                        })
                    }
                    (other, _) => Err(InterpreterError::MethodNotDefined {
                        method: String::from("joinInto"),
                        other_type: get_type(other),
                    }),
                }
            }
        }

        impl<'a> Method for JoinIntoMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if args.len() != 1 {
                    return Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("joinInto"),
                        expected: 1,
                        actual: args.len(),
                    });
                }
                let base_expr = self.outer.eval_single_expr(&p, env)?;
                let source_expr = self.outer.eval_single_expr(&args[0], env)?;
                self.outer
                    .metering
                    .reserve_incremental_primitive(union_cost(1))?;
                let result = self.join_into(&base_expr, &source_expr)?;
                Ok(Par::default().with_exprs(vec![result]))
            }
        }

        Box::new(JoinIntoMethod { outer: self })
    }

    fn at_path_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct AtPathMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> AtPathMethod<'a> {
            fn at_path(&self, base_expr: &Expr, path_par: &Par) -> Result<Par, InterpreterError> {
                match base_expr.expr_instance.clone().unwrap() {
                    ExprInstance::EZipperBody(zipper) => {
                        use models::rust::pathmap_integration::par_to_path;

                        // Get PathMap from zipper
                        let pathmap = zipper.pathmap.expect("zipper pathmap was None");
                        let pathmap_result =
                            PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&pathmap);
                        let rholang_pathmap = pathmap_result.map;

                        // Combine current_path with requested path
                        let path_segments = par_to_path(path_par);
                        let mut full_path = zipper.current_path.clone();
                        full_path.extend(path_segments);

                        // Build key from full path
                        let key: Vec<u8> = segments_to_key(&full_path, true);

                        // Get value at this path
                        match rholang_pathmap.get(&key) {
                            Some(val) => Ok(val.clone()),
                            None => Ok(Par::default()), // Return Nil if not found
                        }
                    }
                    ExprInstance::EPathmapBody(pathmap) => {
                        use models::rust::pathmap_integration::par_to_path;

                        // Get value at path from PathMap root
                        let pathmap_result =
                            PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&pathmap);
                        let rholang_pathmap = pathmap_result.map;

                        let path_segments = par_to_path(path_par);
                        let key: Vec<u8> = segments_to_key(&path_segments, true);

                        match rholang_pathmap.get(&key) {
                            Some(val) => Ok(val.clone()),
                            None => Ok(Par::default()), // Return Nil if not found
                        }
                    }
                    other => Err(InterpreterError::MethodNotDefined {
                        method: String::from("atPath"),
                        other_type: get_type(other),
                    }),
                }
            }
        }

        impl<'a> Method for AtPathMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if args.len() != 1 {
                    return Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("atPath"),
                        expected: 1,
                        actual: args.len(),
                    });
                }
                let base_expr = self.outer.eval_single_expr(&p, env)?;
                let path_par = self.outer.eval_expr(&args[0], env)?;
                self.outer
                    .metering
                    .reserve_incremental_primitive(union_cost(1))?;
                self.at_path(&base_expr, &path_par)
            }
        }

        Box::new(AtPathMethod { outer: self })
    }

    fn path_exists_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct PathExistsMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> PathExistsMethod<'a> {
            fn path_exists(&self, base_expr: &Expr) -> Result<bool, InterpreterError> {
                match base_expr.expr_instance.clone().unwrap() {
                    ExprInstance::EZipperBody(zipper) => {
                        // Get PathMap from zipper
                        let pathmap = zipper.pathmap.as_ref().expect("zipper pathmap was None");
                        let pathmap_result =
                            PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(pathmap);
                        let rholang_pathmap = pathmap_result.map;

                        // Build key from current_path
                        let key: Vec<u8> = segments_to_key(&zipper.current_path, false);

                        // Check if path exists (either has value or has children)
                        if key.is_empty() {
                            // Root always exists if PathMap is not empty
                            Ok(!pathmap.ps.is_empty())
                        } else {
                            // Check if exact path or any path with this prefix exists —
                            // native trie-path lookup (O(path) instead of the previous
                            // whole-map `any(starts_with)` scan; equivalent on the
                            // pure-insert tries produced by create_pathmap_from_elements).
                            Ok(path_prefix_exists(&rholang_pathmap, &key))
                        }
                    }
                    ExprInstance::EPathmapBody(pathmap) => {
                        // For PathMap at root, it exists if not empty
                        Ok(!pathmap.ps.is_empty())
                    }
                    other => Err(InterpreterError::MethodNotDefined {
                        method: String::from("pathExists"),
                        other_type: get_type(other),
                    }),
                }
            }
        }

        impl<'a> Method for PathExistsMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if !args.is_empty() {
                    return Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("pathExists"),
                        expected: 0,
                        actual: args.len(),
                    });
                }
                let base_expr = self.outer.eval_single_expr(&p, env)?;
                self.outer
                    .metering
                    .reserve_incremental_primitive(union_cost(1))?;
                let result = self.path_exists(&base_expr)?;

                // Return as GBool
                Ok(Par::default().with_exprs(vec![Expr {
                    expr_instance: Some(ExprInstance::GBool(result)),
                }]))
            }
        }

        Box::new(PathExistsMethod { outer: self })
    }

    fn create_path_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct CreatePathMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> CreatePathMethod<'a> {
            fn create_path(
                &self,
                base_expr: &Expr,
                path_par: &Par,
            ) -> Result<Expr, InterpreterError> {
                match base_expr.expr_instance.clone().unwrap() {
                    ExprInstance::EZipperBody(zipper) if zipper.is_write_zipper => {
                        use models::rust::pathmap_integration::par_to_path;

                        // Get PathMap from zipper
                        let pathmap = zipper.pathmap.expect("zipper pathmap was None");

                        // Parse requested path to validate format
                        let _path_segments = par_to_path(path_par);

                        // Combine with current path
                        let _ = zipper.current_path.clone(); // Use for future implementation

                        // Create path structure by ensuring intermediate nodes exist
                        // We don't set values, just ensure the path structure exists
                        // In a trie, paths are implicitly created when you add values
                        // Since we want to create structure without values, we'll just
                        // return the PathMap as-is (the structure will be created when needed)
                        // Alternatively, we could insert empty markers but that changes semantics

                        // For now, just return the PathMap unchanged
                        // This is a no-op but validates the path format
                        Ok(Expr {
                            expr_instance: Some(ExprInstance::EPathmapBody(pathmap)),
                        })
                    }
                    ExprInstance::EZipperBody(_) => Err(InterpreterError::MethodNotDefined {
                        method: String::from("createPath (requires write zipper)"),
                        other_type: "read zipper".to_string(),
                    }),
                    other => Err(InterpreterError::MethodNotDefined {
                        method: String::from("createPath"),
                        other_type: get_type(other),
                    }),
                }
            }
        }

        impl<'a> Method for CreatePathMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if args.len() != 1 {
                    return Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("createPath"),
                        expected: 1,
                        actual: args.len(),
                    });
                }
                let base_expr = self.outer.eval_single_expr(&p, env)?;
                let path_par = self.outer.eval_expr(&args[0], env)?;
                self.outer
                    .metering
                    .reserve_incremental_primitive(union_cost(1))?;
                let result = self.create_path(&base_expr, &path_par)?;
                Ok(Par::default().with_exprs(vec![result]))
            }
        }

        Box::new(CreatePathMethod { outer: self })
    }

    fn prune_path_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct PrunePathMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> PrunePathMethod<'a> {
            fn prune_path(&self, base_expr: &Expr) -> Result<Expr, InterpreterError> {
                match base_expr.expr_instance.clone().unwrap() {
                    ExprInstance::EZipperBody(zipper) if zipper.is_write_zipper => {
                        // Get PathMap from zipper
                        let pathmap = zipper.pathmap.expect("zipper pathmap was None");
                        let pathmap_result =
                            PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&pathmap);
                        let mut rholang_pathmap = pathmap_result.map;

                        // Build key from current_path
                        let prefix_key: Vec<u8> = segments_to_key(&zipper.current_path, false);

                        // Remove all entries at and below this path
                        let keys_to_remove: Vec<Vec<u8>> = rholang_pathmap
                            .iter()
                            .filter_map(|(key, _)| {
                                if key.starts_with(&prefix_key) {
                                    Some(key.clone())
                                } else {
                                    None
                                }
                            })
                            .collect();

                        for key in keys_to_remove {
                            rholang_pathmap.remove(&key);
                        }

                        // Convert back to EPathMap
                        let result_pathmap = PathMapCrateTypeMapper::rholang_pathmap_to_e_pathmap(
                            &rholang_pathmap,
                            pathmap_result.connective_used,
                            &pathmap_result.locally_free,
                            None,
                        );

                        Ok(Expr {
                            expr_instance: Some(ExprInstance::EPathmapBody(result_pathmap)),
                        })
                    }
                    ExprInstance::EZipperBody(_) => Err(InterpreterError::MethodNotDefined {
                        method: String::from("prunePath (requires write zipper)"),
                        other_type: "read zipper".to_string(),
                    }),
                    other => Err(InterpreterError::MethodNotDefined {
                        method: String::from("prunePath"),
                        other_type: get_type(other),
                    }),
                }
            }
        }

        impl<'a> Method for PrunePathMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if !args.is_empty() {
                    return Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("prunePath"),
                        expected: 0,
                        actual: args.len(),
                    });
                }
                let base_expr = self.outer.eval_single_expr(&p, env)?;
                self.outer.metering.reserve_primitive(remove_cost())?;
                let result = self.prune_path(&base_expr)?;
                Ok(Par::default().with_exprs(vec![result]))
            }
        }

        Box::new(PrunePathMethod { outer: self })
    }

    fn reset_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct ResetMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> ResetMethod<'a> {
            fn reset(&self, base_expr: &Expr) -> Result<Expr, InterpreterError> {
                match base_expr.expr_instance.clone().unwrap() {
                    ExprInstance::EZipperBody(mut zipper) => {
                        // Reset to root by clearing current_path
                        zipper.current_path = vec![];

                        Ok(Expr {
                            expr_instance: Some(ExprInstance::EZipperBody(zipper)),
                        })
                    }
                    other => Err(InterpreterError::MethodNotDefined {
                        method: String::from("reset"),
                        other_type: get_type(other),
                    }),
                }
            }
        }

        impl<'a> Method for ResetMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if !args.is_empty() {
                    return Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("reset"),
                        expected: 0,
                        actual: args.len(),
                    });
                }
                let base_expr = self.outer.eval_single_expr(&p, env)?;
                self.outer
                    .metering
                    .reserve_incremental_primitive(union_cost(1))?;
                let result = self.reset(&base_expr)?;
                Ok(Par::default().with_exprs(vec![result]))
            }
        }

        Box::new(ResetMethod { outer: self })
    }

    fn ascend_one_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct AscendOneMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> AscendOneMethod<'a> {
            fn ascend_one(&self, base_expr: &Expr) -> Result<Par, InterpreterError> {
                match base_expr.expr_instance.clone().unwrap() {
                    ExprInstance::EZipperBody(mut zipper) => {
                        // Check if at root
                        if zipper.current_path.is_empty() {
                            // At root, cannot ascend - return Nil
                            return Ok(Par::default());
                        }

                        // Remove last segment from current_path (ascend one level)
                        zipper.current_path.pop();

                        Ok(Par::default().with_exprs(vec![Expr {
                            expr_instance: Some(ExprInstance::EZipperBody(zipper)),
                        }]))
                    }
                    other => Err(InterpreterError::MethodNotDefined {
                        method: String::from("ascendOne"),
                        other_type: get_type(other),
                    }),
                }
            }
        }

        impl<'a> Method for AscendOneMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if !args.is_empty() {
                    return Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("ascendOne"),
                        expected: 0,
                        actual: args.len(),
                    });
                }
                let base_expr = self.outer.eval_single_expr(&p, env)?;
                self.outer
                    .metering
                    .reserve_incremental_primitive(union_cost(1))?;
                self.ascend_one(&base_expr)
            }
        }

        Box::new(AscendOneMethod { outer: self })
    }

    fn ascend_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct AscendMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> AscendMethod<'a> {
            fn ascend(&self, base_expr: &Expr, steps_par: &Par) -> Result<Par, InterpreterError> {
                // Extract integer from Par
                let steps = match steps_par
                    .exprs
                    .first()
                    .and_then(|e| e.expr_instance.as_ref())
                {
                    Some(ExprInstance::GInt(n)) => *n,
                    _ => {
                        return Err(InterpreterError::MethodNotDefined {
                            method: String::from("ascend (requires integer argument)"),
                            other_type: "non-integer".to_string(),
                        })
                    }
                };

                if steps < 0 {
                    return Err(InterpreterError::MethodNotDefined {
                        method: String::from("ascend (steps must be non-negative)"),
                        other_type: format!("negative: {}", steps),
                    });
                }

                match base_expr.expr_instance.clone().unwrap() {
                    ExprInstance::EZipperBody(mut zipper) => {
                        // Remove up to 'steps' segments, cap at root
                        let depth = zipper.current_path.len();
                        let actual_steps = std::cmp::min(steps as usize, depth);

                        // Remove segments from end
                        for _ in 0..actual_steps {
                            zipper.current_path.pop();
                        }

                        Ok(Par::default().with_exprs(vec![Expr {
                            expr_instance: Some(ExprInstance::EZipperBody(zipper)),
                        }]))
                    }
                    other => Err(InterpreterError::MethodNotDefined {
                        method: String::from("ascend"),
                        other_type: get_type(other),
                    }),
                }
            }
        }

        impl<'a> Method for AscendMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if args.len() != 1 {
                    return Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("ascend"),
                        expected: 1,
                        actual: args.len(),
                    });
                }
                let base_expr = self.outer.eval_single_expr(&p, env)?;
                let steps_par = self.outer.eval_expr(&args[0], env)?;
                self.outer
                    .metering
                    .reserve_incremental_primitive(union_cost(1))?;
                self.ascend(&base_expr, &steps_par)
            }
        }

        Box::new(AscendMethod { outer: self })
    }

    fn child_count_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct ChildCountMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> ChildCountMethod<'a> {
            fn child_count(&self, base_expr: &Expr) -> Result<i64, InterpreterError> {
                match base_expr.expr_instance.clone().unwrap() {
                    ExprInstance::EZipperBody(zipper) => {
                        let pathmap = zipper.pathmap.as_ref().expect("zipper pathmap was None");
                        let pathmap_result =
                            PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(pathmap);
                        let rholang_pathmap = pathmap_result.map;

                        // Build prefix from current_path
                        let prefix_key: Vec<u8> = segments_to_key(&zipper.current_path, false);

                        // Find all unique immediate children — native trie descent
                        // (O(prefix + distinct child segments) instead of the previous
                        // whole-map scan). The helper emits distinct segments already in
                        // the ascending byte-lex order the scan's sort()+dedup() produced.
                        let children = collect_child_segments(&rholang_pathmap, &prefix_key, None);

                        Ok(children.len() as i64)
                    }
                    ExprInstance::EPathmapBody(pathmap) => {
                        // For PathMap at root, count top-level paths
                        let pathmap_result =
                            PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&pathmap);
                        let rholang_pathmap = pathmap_result.map;

                        // Distinct first segments = child segments below the empty prefix
                        // (native descent; same result set and order as the retired
                        // whole-map first-segment scan + sort()+dedup()).
                        let children = collect_child_segments(&rholang_pathmap, &[], None);

                        Ok(children.len() as i64)
                    }
                    other => Err(InterpreterError::MethodNotDefined {
                        method: String::from("childCount"),
                        other_type: get_type(other),
                    }),
                }
            }
        }

        impl<'a> Method for ChildCountMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if !args.is_empty() {
                    return Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("childCount"),
                        expected: 0,
                        actual: args.len(),
                    });
                }
                let base_expr = self.outer.eval_single_expr(&p, env)?;
                self.outer
                    .metering
                    .reserve_incremental_primitive(union_cost(1))?;
                let count = self.child_count(&base_expr)?;

                Ok(Par::default().with_exprs(vec![Expr {
                    expr_instance: Some(ExprInstance::GInt(count)),
                }]))
            }
        }

        Box::new(ChildCountMethod { outer: self })
    }

    fn descend_first_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct DescendFirstMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> DescendFirstMethod<'a> {
            fn descend_first(&self, base_expr: &Expr) -> Result<Par, InterpreterError> {
                match base_expr.expr_instance.clone().unwrap() {
                    ExprInstance::EZipperBody(mut zipper) => {
                        let pathmap = zipper.pathmap.as_ref().expect("zipper pathmap was None");
                        let pathmap_result =
                            PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(pathmap);
                        let rholang_pathmap = pathmap_result.map;

                        // Build prefix from current_path
                        let prefix_key: Vec<u8> = segments_to_key(&zipper.current_path, false);

                        // Find the first (byte-lex smallest) immediate child — native
                        // trie descent with early stop after one emission (O(prefix +
                        // first segment) instead of the previous whole-map scan +
                        // sort()+dedup(); the helper emits in exactly that sorted order,
                        // so the first emission IS the retired `children.first()`).
                        let children = collect_child_segments(&rholang_pathmap, &prefix_key, Some(1));

                        // Get first child
                        if let Some(first_child) = children.first() {
                            zipper.current_path.push(first_child.clone());
                            Ok(Par::default().with_exprs(vec![Expr {
                                expr_instance: Some(ExprInstance::EZipperBody(zipper)),
                            }]))
                        } else {
                            // No children, return Nil
                            Ok(Par::default())
                        }
                    }
                    other => Err(InterpreterError::MethodNotDefined {
                        method: String::from("descendFirst"),
                        other_type: get_type(other),
                    }),
                }
            }
        }

        impl<'a> Method for DescendFirstMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if !args.is_empty() {
                    return Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("descendFirst"),
                        expected: 0,
                        actual: args.len(),
                    });
                }
                let base_expr = self.outer.eval_single_expr(&p, env)?;
                self.outer
                    .metering
                    .reserve_incremental_primitive(union_cost(1))?;
                self.descend_first(&base_expr)
            }
        }

        Box::new(DescendFirstMethod { outer: self })
    }

    fn descend_indexed_branch_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct DescendIndexedBranchMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> DescendIndexedBranchMethod<'a> {
            fn descend_indexed(
                &self,
                base_expr: &Expr,
                idx_par: &Par,
            ) -> Result<Par, InterpreterError> {
                // Extract integer index
                let idx = match idx_par.exprs.first().and_then(|e| e.expr_instance.as_ref()) {
                    Some(ExprInstance::GInt(n)) => *n,
                    _ => {
                        return Err(InterpreterError::MethodNotDefined {
                            method: String::from(
                                "descendIndexedBranch (requires integer argument)",
                            ),
                            other_type: "non-integer".to_string(),
                        })
                    }
                };

                if idx < 0 {
                    // Negative index, return Nil
                    return Ok(Par::default());
                }

                match base_expr.expr_instance.clone().unwrap() {
                    ExprInstance::EZipperBody(mut zipper) => {
                        let pathmap = zipper.pathmap.as_ref().expect("zipper pathmap was None");
                        let pathmap_result =
                            PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(pathmap);
                        let rholang_pathmap = pathmap_result.map;

                        // Build prefix from current_path
                        let prefix_key: Vec<u8> = segments_to_key(&zipper.current_path, false);

                        // Find the idx-th immediate child in ascending byte-lex order —
                        // native trie descent with early stop after idx+1 emissions
                        // (O(prefix + first idx+1 segments) instead of the previous
                        // whole-map scan + sort()+dedup(); the helper emits in exactly
                        // that sorted, distinct order).
                        // (saturating_add: a saturated limit simply enumerates every
                        // child, and `.get(idx)` still yields None — the retired scan's
                        // out-of-bounds behavior.)
                        let children = collect_child_segments(
                            &rholang_pathmap,
                            &prefix_key,
                            Some((idx as usize).saturating_add(1)),
                        );

                        // Get child at index
                        if let Some(child) = children.get(idx as usize) {
                            zipper.current_path.push(child.clone());
                            Ok(Par::default().with_exprs(vec![Expr {
                                expr_instance: Some(ExprInstance::EZipperBody(zipper)),
                            }]))
                        } else {
                            // Index out of bounds, return Nil
                            Ok(Par::default())
                        }
                    }
                    other => Err(InterpreterError::MethodNotDefined {
                        method: String::from("descendIndexedBranch"),
                        other_type: get_type(other),
                    }),
                }
            }
        }

        impl<'a> Method for DescendIndexedBranchMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if args.len() != 1 {
                    return Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("descendIndexedBranch"),
                        expected: 1,
                        actual: args.len(),
                    });
                }
                let base_expr = self.outer.eval_single_expr(&p, env)?;
                let idx_par = self.outer.eval_expr(&args[0], env)?;
                self.outer
                    .metering
                    .reserve_incremental_primitive(union_cost(1))?;
                self.descend_indexed(&base_expr, &idx_par)
            }
        }

        Box::new(DescendIndexedBranchMethod { outer: self })
    }

    fn to_next_sibling_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct ToNextSiblingMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> ToNextSiblingMethod<'a> {
            fn to_next_sibling(&self, base_expr: &Expr) -> Result<Par, InterpreterError> {
                match base_expr.expr_instance.clone().unwrap() {
                    ExprInstance::EZipperBody(mut zipper) => {
                        // Check if at root (no siblings at root)
                        if zipper.current_path.is_empty() {
                            return Ok(Par::default());
                        }

                        let pathmap = zipper.pathmap.as_ref().expect("zipper pathmap was None");
                        let pathmap_result =
                            PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(pathmap);
                        let rholang_pathmap = pathmap_result.map;

                        // Get parent path and current segment
                        let current_segment = zipper.current_path.last().unwrap().clone();
                        let parent_path = &zipper.current_path[..zipper.current_path.len() - 1];
                        let parent_key: Vec<u8> = segments_to_key(parent_path, false);

                        // Find all siblings (children of parent) — native trie descent
                        // (O(parent + distinct siblings) instead of the previous
                        // whole-map scan; emitted in the same ascending byte-lex,
                        // deduplicated order the scan's sort()+dedup() produced).
                        let siblings = collect_child_segments(&rholang_pathmap, &parent_key, None);

                        // Find current position and get next
                        if let Some(current_idx) =
                            siblings.iter().position(|s| s == &current_segment)
                        {
                            if current_idx + 1 < siblings.len() {
                                // Replace current segment with next sibling
                                zipper.current_path.pop();
                                zipper.current_path.push(siblings[current_idx + 1].clone());
                                Ok(Par::default().with_exprs(vec![Expr {
                                    expr_instance: Some(ExprInstance::EZipperBody(zipper)),
                                }]))
                            } else {
                                // No next sibling, return Nil
                                Ok(Par::default())
                            }
                        } else {
                            // Current not found (shouldn't happen), return Nil
                            Ok(Par::default())
                        }
                    }
                    other => Err(InterpreterError::MethodNotDefined {
                        method: String::from("toNextSibling"),
                        other_type: get_type(other),
                    }),
                }
            }
        }

        impl<'a> Method for ToNextSiblingMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if !args.is_empty() {
                    return Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("toNextSibling"),
                        expected: 0,
                        actual: args.len(),
                    });
                }
                let base_expr = self.outer.eval_single_expr(&p, env)?;
                self.outer
                    .metering
                    .reserve_incremental_primitive(union_cost(1))?;
                self.to_next_sibling(&base_expr)
            }
        }

        Box::new(ToNextSiblingMethod { outer: self })
    }

    fn to_prev_sibling_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct ToPrevSiblingMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> ToPrevSiblingMethod<'a> {
            fn to_prev_sibling(&self, base_expr: &Expr) -> Result<Par, InterpreterError> {
                match base_expr.expr_instance.clone().unwrap() {
                    ExprInstance::EZipperBody(mut zipper) => {
                        // Check if at root (no siblings at root)
                        if zipper.current_path.is_empty() {
                            return Ok(Par::default());
                        }

                        let pathmap = zipper.pathmap.as_ref().expect("zipper pathmap was None");
                        let pathmap_result =
                            PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(pathmap);
                        let rholang_pathmap = pathmap_result.map;

                        // Get parent path and current segment
                        let current_segment = zipper.current_path.last().unwrap().clone();
                        let parent_path = &zipper.current_path[..zipper.current_path.len() - 1];
                        let parent_key: Vec<u8> = segments_to_key(parent_path, false);

                        // Find all siblings (children of parent) — native trie descent
                        // (O(parent + distinct siblings) instead of the previous
                        // whole-map scan; emitted in the same ascending byte-lex,
                        // deduplicated order the scan's sort()+dedup() produced).
                        let siblings = collect_child_segments(&rholang_pathmap, &parent_key, None);

                        // Find current position and get previous
                        if let Some(current_idx) =
                            siblings.iter().position(|s| s == &current_segment)
                        {
                            if current_idx > 0 {
                                // Replace current segment with previous sibling
                                zipper.current_path.pop();
                                zipper.current_path.push(siblings[current_idx - 1].clone());
                                Ok(Par::default().with_exprs(vec![Expr {
                                    expr_instance: Some(ExprInstance::EZipperBody(zipper)),
                                }]))
                            } else {
                                // No previous sibling, return Nil
                                Ok(Par::default())
                            }
                        } else {
                            // Current not found (shouldn't happen), return Nil
                            Ok(Par::default())
                        }
                    }
                    other => Err(InterpreterError::MethodNotDefined {
                        method: String::from("toPrevSibling"),
                        other_type: get_type(other),
                    }),
                }
            }
        }

        impl<'a> Method for ToPrevSiblingMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if !args.is_empty() {
                    return Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("toPrevSibling"),
                        expected: 0,
                        actual: args.len(),
                    });
                }
                let base_expr = self.outer.eval_single_expr(&p, env)?;
                self.outer
                    .metering
                    .reserve_incremental_primitive(union_cost(1))?;
                self.to_prev_sibling(&base_expr)
            }
        }

        Box::new(ToPrevSiblingMethod { outer: self })
    }

    // ============ END ZIPPER METHODS ============

    fn add_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct AddMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> AddMethod<'a> {
            fn add(&self, base_expr: Expr, par: Par) -> Result<Expr, InterpreterError> {
                match base_expr.expr_instance {
                    Some(expr_instance) => match expr_instance {
                        ExprInstance::ESetBody(eset) => {
                            let base = ParSetTypeMapper::eset_to_par_set(eset);
                            let mut base_ps = base.ps;

                            Ok(Expr {
                                expr_instance: Some(ExprInstance::ESetBody(
                                    ParSetTypeMapper::par_set_to_eset(ParSet {
                                        ps: base_ps.insert(par.clone()),
                                        connective_used: base.connective_used
                                            || par.connective_used,
                                        locally_free: union(base.locally_free, par.locally_free),
                                        remainder: None,
                                    }),
                                )),
                            })
                        }

                        other => Err(InterpreterError::MethodNotDefined {
                            method: String::from("add"),
                            other_type: get_type(other),
                        }),
                    },

                    None => Err(InterpreterError::MethodNotDefined {
                        method: String::from("add"),
                        other_type: String::from("None"),
                    }),
                }
            }
        }

        impl<'a> Method for AddMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if args.len() != 1 {
                    Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("add"),
                        expected: 1,
                        actual: args.len(),
                    })
                } else {
                    let base_expr = self.outer.eval_single_expr(&p, env)?;
                    let element = self.outer.eval_expr(&args[0], env)?;
                    self.outer.metering.reserve_primitive(add_cost())?;
                    let result = self.add(base_expr, element)?;
                    Ok(Par::default().with_exprs(vec![result]))
                }
            }
        }

        Box::new(AddMethod { outer: self })
    }

    fn delete_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct DeleteMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> DeleteMethod<'a> {
            fn delete(&self, base_expr: Expr, par: Par) -> Result<Expr, InterpreterError> {
                match base_expr.expr_instance {
                    Some(expr_instance) => match expr_instance {
                        ExprInstance::ESetBody(eset) => {
                            let base = ParSetTypeMapper::eset_to_par_set(eset);
                            let mut base_ps = base.ps;

                            Ok(Expr {
                                expr_instance: Some(ExprInstance::ESetBody(
                                    ParSetTypeMapper::par_set_to_eset(ParSet {
                                        ps: base_ps.remove(par.clone()),
                                        connective_used: base.connective_used
                                            || par.connective_used,
                                        locally_free: union(base.locally_free, par.locally_free),
                                        remainder: None,
                                    }),
                                )),
                            })
                        }

                        ExprInstance::EMapBody(emap) => {
                            let base = ParMapTypeMapper::emap_to_par_map(emap);
                            let mut base_ps = base.ps;

                            Ok(Expr {
                                expr_instance: Some(ExprInstance::EMapBody(
                                    ParMapTypeMapper::par_map_to_emap(ParMap {
                                        ps: base_ps.remove(par.clone()),
                                        connective_used: base.connective_used
                                            || par.connective_used,
                                        locally_free: union(base.locally_free, par.locally_free),
                                        remainder: None,
                                    }),
                                )),
                            })
                        }

                        other => Err(InterpreterError::MethodNotDefined {
                            method: String::from("delete"),
                            other_type: get_type(other),
                        }),
                    },

                    None => Err(InterpreterError::MethodNotDefined {
                        method: String::from("delete"),
                        other_type: String::from("None"),
                    }),
                }
            }
        }

        impl<'a> Method for DeleteMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if args.len() != 1 {
                    Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("delete"),
                        expected: 1,
                        actual: args.len(),
                    })
                } else {
                    let base_expr = self.outer.eval_single_expr(&p, env)?;
                    let element = self.outer.eval_expr(&args[0], env)?;
                    //TODO(mateusz.gorski): think whether deletion of an element from the collection should dependent on the collection type/size - OLD
                    self.outer.metering.reserve_primitive(remove_cost())?;
                    let result = self.delete(base_expr, element)?;
                    Ok(Par::default().with_exprs(vec![result]))
                }
            }
        }

        Box::new(DeleteMethod { outer: self })
    }

    fn contains_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct ContainsMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> ContainsMethod<'a> {
            fn contains(&self, base_expr: Expr, par: Par) -> Result<Expr, InterpreterError> {
                match base_expr.expr_instance {
                    Some(expr_instance) => match expr_instance {
                        ExprInstance::ESetBody(eset) => {
                            let base_ps = ParSetTypeMapper::eset_to_par_set(eset).ps;

                            Ok(Expr {
                                expr_instance: Some(ExprInstance::GBool(base_ps.contains(par))),
                            })
                        }

                        ExprInstance::EMapBody(emap) => {
                            let base_ps = ParMapTypeMapper::emap_to_par_map(emap).ps;

                            Ok(Expr {
                                expr_instance: Some(ExprInstance::GBool(base_ps.contains(par))),
                            })
                        }

                        other => Err(InterpreterError::MethodNotDefined {
                            method: String::from("contains"),
                            other_type: get_type(other),
                        }),
                    },

                    None => Err(InterpreterError::MethodNotDefined {
                        method: String::from("contains"),
                        other_type: String::from("None"),
                    }),
                }
            }
        }

        impl<'a> Method for ContainsMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if args.len() != 1 {
                    Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("contains"),
                        expected: 1,
                        actual: args.len(),
                    })
                } else {
                    let base_expr = self.outer.eval_single_expr(&p, env)?;
                    let element = self.outer.eval_expr(&args[0], env)?;
                    self.outer.metering.reserve_primitive(lookup_cost())?;
                    let result = self.contains(base_expr, element)?;
                    Ok(Par::default().with_exprs(vec![result]))
                }
            }
        }

        Box::new(ContainsMethod { outer: self })
    }

    fn get_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct GetMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> GetMethod<'a> {
            fn get(&self, base_expr: Expr, key: Par) -> Result<Par, InterpreterError> {
                match base_expr.expr_instance {
                    Some(expr_instance) => match expr_instance {
                        ExprInstance::EMapBody(emap) => {
                            let base_ps = ParMapTypeMapper::emap_to_par_map(emap).ps;
                            Ok(base_ps.get_or_else(key, Par::default()))
                        }

                        other => Err(InterpreterError::MethodNotDefined {
                            method: String::from("get"),
                            other_type: get_type(other),
                        }),
                    },

                    None => Err(InterpreterError::MethodNotDefined {
                        method: String::from("get"),
                        other_type: String::from("None"),
                    }),
                }
            }
        }

        impl<'a> Method for GetMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if args.len() != 1 {
                    Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("get"),
                        expected: 1,
                        actual: args.len(),
                    })
                } else {
                    let base_expr = self.outer.eval_single_expr(&p, env)?;
                    let key = self.outer.eval_expr(&args[0], env)?;
                    self.outer.metering.reserve_primitive(lookup_cost())?;
                    let result = self.get(base_expr, key)?;
                    Ok(result)
                }
            }
        }

        Box::new(GetMethod { outer: self })
    }

    fn get_or_else_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct GetOrElseMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> GetOrElseMethod<'a> {
            fn get_or_else(
                &self,
                base_expr: Expr,
                key: Par,
                default: Par,
            ) -> Result<Par, InterpreterError> {
                match base_expr.expr_instance {
                    Some(expr_instance) => match expr_instance {
                        ExprInstance::EMapBody(emap) => {
                            let base_ps = ParMapTypeMapper::emap_to_par_map(emap).ps;
                            Ok(base_ps.get_or_else(key, default))
                        }

                        other => Err(InterpreterError::MethodNotDefined {
                            method: String::from("get_or_else"),
                            other_type: get_type(other),
                        }),
                    },

                    None => Err(InterpreterError::MethodNotDefined {
                        method: String::from("get_or_else"),
                        other_type: String::from("None"),
                    }),
                }
            }
        }

        impl<'a> Method for GetOrElseMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if args.len() != 2 {
                    Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("get_or_else"),
                        expected: 2,
                        actual: args.len(),
                    })
                } else {
                    let base_expr = self.outer.eval_single_expr(&p, env)?;
                    let key = self.outer.eval_expr(&args[0], env)?;
                    let default = self.outer.eval_expr(&args[1], env)?;
                    self.outer.metering.reserve_primitive(lookup_cost())?;
                    let result = self.get_or_else(base_expr, key, default)?;
                    Ok(result)
                }
            }
        }

        Box::new(GetOrElseMethod { outer: self })
    }

    fn set_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct SetMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> SetMethod<'a> {
            fn set(&self, base_expr: Expr, key: Par, value: Par) -> Result<Par, InterpreterError> {
                match base_expr.expr_instance {
                    Some(expr_instance) => match expr_instance {
                        ExprInstance::EMapBody(emap) => {
                            let mut base_ps = ParMapTypeMapper::emap_to_par_map(emap).ps;
                            // let sorted_par_map = base_ps.insert((key, value));
                            let par_map =
                                ParMap::create_from_sorted_par_map(base_ps.insert((key, value)));

                            Ok(Par::default().with_exprs(vec![Expr {
                                expr_instance: Some(ExprInstance::EMapBody(
                                    ParMapTypeMapper::par_map_to_emap(par_map),
                                )),
                            }]))
                        }

                        other => Err(InterpreterError::MethodNotDefined {
                            method: String::from("set"),
                            other_type: get_type(other),
                        }),
                    },

                    None => Err(InterpreterError::MethodNotDefined {
                        method: String::from("set"),
                        other_type: String::from("None"),
                    }),
                }
            }
        }

        impl<'a> Method for SetMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if args.len() != 2 {
                    Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("set"),
                        expected: 2,
                        actual: args.len(),
                    })
                } else {
                    let base_expr = self.outer.eval_single_expr(&p, env)?;
                    let key = self.outer.eval_expr(&args[0], env)?;
                    let value = self.outer.eval_expr(&args[1], env)?;
                    self.outer.metering.reserve_primitive(add_cost())?;
                    let result = self.set(base_expr, key, value)?;
                    Ok(result)
                }
            }
        }

        Box::new(SetMethod { outer: self })
    }

    fn keys_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct KeysMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> KeysMethod<'a> {
            fn keys(&self, base_expr: Expr) -> Result<Par, InterpreterError> {
                match base_expr.expr_instance {
                    Some(expr_instance) => match expr_instance {
                        ExprInstance::EMapBody(emap) => {
                            let base_ps = ParMapTypeMapper::emap_to_par_map(emap).ps;
                            let par_set = ParSet::create_from_vec(base_ps.keys());

                            Ok(Par::default().with_exprs(vec![Expr {
                                expr_instance: Some(ExprInstance::ESetBody(
                                    ParSetTypeMapper::par_set_to_eset(par_set),
                                )),
                            }]))
                        }

                        other => Err(InterpreterError::MethodNotDefined {
                            method: String::from("keys"),
                            other_type: get_type(other),
                        }),
                    },

                    None => Err(InterpreterError::MethodNotDefined {
                        method: String::from("keys"),
                        other_type: String::from("None"),
                    }),
                }
            }
        }

        impl<'a> Method for KeysMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if !args.is_empty() {
                    Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("keys"),
                        expected: 0,
                        actual: args.len(),
                    })
                } else {
                    let base_expr = self.outer.eval_single_expr(&p, env)?;
                    self.outer.metering.reserve_primitive(keys_method_cost())?;
                    let result = self.keys(base_expr)?;
                    Ok(result)
                }
            }
        }

        Box::new(KeysMethod { outer: self })
    }

    fn size_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct SizeMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> SizeMethod<'a> {
            fn size(&self, base_expr: Expr) -> Result<(i64, Par), InterpreterError> {
                match base_expr.expr_instance {
                    Some(expr_instance) => match expr_instance {
                        ExprInstance::EMapBody(emap) => {
                            let base_ps = ParMapTypeMapper::emap_to_par_map(emap).ps;
                            let size = base_ps.length() as i64;

                            Ok((size, new_gint_par(size, Vec::new(), false)))
                        }

                        ExprInstance::ESetBody(eset) => {
                            let base_ps = ParSetTypeMapper::eset_to_par_set(eset).ps;
                            let size = base_ps.length() as i64;

                            Ok((size, new_gint_par(size, Vec::new(), false)))
                        }

                        other => Err(InterpreterError::MethodNotDefined {
                            method: String::from("size"),
                            other_type: get_type(other),
                        }),
                    },

                    None => Err(InterpreterError::MethodNotDefined {
                        method: String::from("size"),
                        other_type: String::from("None"),
                    }),
                }
            }
        }

        impl<'a> Method for SizeMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if !args.is_empty() {
                    Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("size"),
                        expected: 0,
                        actual: args.len(),
                    })
                } else {
                    let base_expr = self.outer.eval_single_expr(&p, env)?;
                    let result = self.size(base_expr)?;
                    self.outer
                        .metering
                        .reserve_incremental_primitive(size_method_cost(result.0))?;
                    Ok(result.1)
                }
            }
        }

        Box::new(SizeMethod { outer: self })
    }

    fn length_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct LengthMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> LengthMethod<'a> {
            fn length(&self, base_expr: Expr) -> Result<Expr, InterpreterError> {
                match base_expr.expr_instance {
                    Some(expr_instance) => match expr_instance {
                        ExprInstance::GString(string) => Ok(new_gint_expr(string.len() as i64)),

                        ExprInstance::GByteArray(bytes) => Ok(new_gint_expr(bytes.len() as i64)),

                        ExprInstance::EListBody(elist) => Ok(new_gint_expr(elist.ps.len() as i64)),

                        other => Err(InterpreterError::MethodNotDefined {
                            method: String::from("length"),
                            other_type: get_type(other),
                        }),
                    },

                    None => Err(InterpreterError::MethodNotDefined {
                        method: String::from("length"),
                        other_type: String::from("None"),
                    }),
                }
            }
        }

        impl<'a> Method for LengthMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if !args.is_empty() {
                    Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("length"),
                        expected: 0,
                        actual: args.len(),
                    })
                } else {
                    let base_expr = self.outer.eval_single_expr(&p, env)?;
                    self.outer
                        .metering
                        .reserve_primitive(length_method_cost())?;
                    let result = self.length(base_expr)?;
                    Ok(Par::default().with_exprs(vec![result]))
                }
            }
        }

        Box::new(LengthMethod { outer: self })
    }

    fn slice_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct SliceMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> SliceMethod<'a> {
            fn slice(
                &self,
                base_expr: Expr,
                from: usize,
                until: usize,
            ) -> Result<Par, InterpreterError> {
                match base_expr.expr_instance {
                    Some(expr_instance) => match expr_instance {
                        ExprInstance::GString(string) => Ok(new_gstring_par(
                            if from <= until && until <= string.len() {
                                string[from..until].to_string()
                            } else {
                                "".to_string()
                            },
                            Vec::new(),
                            false,
                        )),

                        ExprInstance::EListBody(elist) => Ok(new_elist_par(
                            if from <= until && until <= elist.ps.len() {
                                elist.ps[from..until].to_vec()
                            } else {
                                vec![]
                            },
                            elist.locally_free,
                            elist.connective_used,
                            elist.remainder,
                            Vec::new(),
                            false,
                        )),

                        ExprInstance::GByteArray(bytes) => {
                            Ok(Par::default().with_exprs(vec![Expr {
                                expr_instance: Some(ExprInstance::GByteArray(
                                    if from <= until && until <= bytes.len() {
                                        bytes[from..until].to_vec()
                                    } else {
                                        vec![]
                                    },
                                )),
                            }]))
                        }

                        other => Err(InterpreterError::MethodNotDefined {
                            method: String::from("slice"),
                            other_type: get_type(other),
                        }),
                    },

                    None => Err(InterpreterError::MethodNotDefined {
                        method: String::from("slice"),
                        other_type: String::from("None"),
                    }),
                }
            }
        }

        impl<'a> Method for SliceMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if args.len() != 2 {
                    Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("slice"),
                        expected: 2,
                        actual: args.len(),
                    })
                } else {
                    let base_expr = self.outer.eval_single_expr(&p, env)?;
                    let from_arg = self.outer.eval_to_i64(&args[0], env)?;
                    let to_arg = self.outer.eval_to_i64(&args[1], env)?;
                    let from = from_arg.max(0) as usize;
                    let until = to_arg.max(0) as usize;
                    self.outer
                        .metering
                        .reserve_incremental_primitive(slice_cost(until as i64))?;
                    let result = self.slice(base_expr, from, until)?;
                    Ok(result)
                }
            }
        }

        Box::new(SliceMethod { outer: self })
    }

    fn take_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct TakeMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> TakeMethod<'a> {
            fn take(&self, base_expr: Expr, n: usize) -> Result<Par, InterpreterError> {
                match base_expr.expr_instance {
                    Some(expr_instance) => match expr_instance {
                        ExprInstance::EListBody(elist) => Ok(new_elist_par(
                            elist.ps.into_iter().take(n).collect(),
                            elist.locally_free,
                            elist.connective_used,
                            elist.remainder,
                            Vec::new(),
                            false,
                        )),

                        other => Err(InterpreterError::MethodNotDefined {
                            method: String::from("take"),
                            other_type: get_type(other),
                        }),
                    },

                    None => Err(InterpreterError::MethodNotDefined {
                        method: String::from("take"),
                        other_type: String::from("None"),
                    }),
                }
            }
        }

        impl<'a> Method for TakeMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if args.len() != 1 {
                    Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("take"),
                        expected: 1,
                        actual: args.len(),
                    })
                } else {
                    let base_expr = self.outer.eval_single_expr(&p, env)?;
                    let n_arg = self.outer.eval_to_i64(&args[0], env)?;
                    let n = n_arg.max(0) as usize;
                    self.outer
                        .metering
                        .reserve_incremental_primitive(take_cost(n as i64))?;
                    let result = self.take(base_expr, n)?;
                    Ok(result)
                }
            }
        }

        Box::new(TakeMethod { outer: self })
    }

    fn to_list_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct ToListMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> ToListMethod<'a> {
            fn to_list(&self, base_expr: Expr) -> Result<Par, InterpreterError> {
                match base_expr.expr_instance {
                    Some(expr_instance) => match expr_instance {
                        ExprInstance::EListBody(elist) => {
                            Ok(Par::default().with_exprs(vec![Expr {
                                expr_instance: Some(ExprInstance::EListBody(elist)),
                            }]))
                        }

                        ExprInstance::ESetBody(eset) => {
                            let ps = ParSetTypeMapper::eset_to_par_set(eset).ps;
                            self.outer
                                .metering
                                .reserve_incremental_primitive(to_list_cost(ps.length() as i64))?;

                            Ok(Par::default().with_exprs(vec![Expr {
                                expr_instance: Some(ExprInstance::EListBody(EList {
                                    ps: ps.sorted_pars,
                                    locally_free: Vec::new(),
                                    connective_used: false,
                                    remainder: None,
                                })),
                            }]))
                        }

                        ExprInstance::EMapBody(emap) => {
                            let ps = ParMapTypeMapper::emap_to_par_map(emap).ps;
                            self.outer
                                .metering
                                .reserve_incremental_primitive(to_list_cost(ps.length() as i64))?;

                            Ok(Par::default().with_exprs(vec![Expr {
                                expr_instance: Some(ExprInstance::EListBody(EList {
                                    ps: ps
                                        .sorted_list
                                        .into_iter()
                                        .map(|(k, v)| {
                                            Par::default().with_exprs(vec![Expr {
                                                expr_instance: Some(ExprInstance::ETupleBody(
                                                    ETuple {
                                                        ps: vec![k, v],
                                                        locally_free: Vec::new(),
                                                        connective_used: false,
                                                    },
                                                )),
                                            }])
                                        })
                                        .collect(),
                                    locally_free: Vec::new(),
                                    connective_used: false,
                                    remainder: None,
                                })),
                            }]))
                        }

                        ExprInstance::ETupleBody(etuple) => {
                            let ps = etuple.ps;
                            self.outer
                                .metering
                                .reserve_incremental_primitive(to_list_cost(ps.len() as i64))?;

                            Ok(Par::default().with_exprs(vec![Expr {
                                expr_instance: Some(ExprInstance::EListBody(EList {
                                    ps,
                                    locally_free: Vec::new(),
                                    connective_used: false,
                                    remainder: None,
                                })),
                            }]))
                        }

                        other => Err(InterpreterError::MethodNotDefined {
                            method: String::from("to_list"),
                            other_type: get_type(other),
                        }),
                    },

                    None => Err(InterpreterError::MethodNotDefined {
                        method: String::from("to_list"),
                        other_type: String::from("None"),
                    }),
                }
            }
        }

        impl<'a> Method for ToListMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if !args.is_empty() {
                    Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("to_list"),
                        expected: 0,
                        actual: args.len(),
                    })
                } else {
                    let base_expr = self.outer.eval_single_expr(&p, env)?;
                    let result = self.to_list(base_expr)?;
                    Ok(result)
                }
            }
        }

        Box::new(ToListMethod { outer: self })
    }

    fn to_set_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct ToSetMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> ToSetMethod<'a> {
            fn to_set(&self, base_expr: Expr) -> Result<Par, InterpreterError> {
                match base_expr.expr_instance {
                    Some(expr_instance) => match expr_instance {
                        ExprInstance::ESetBody(eset) => Ok(Par::default().with_exprs(vec![Expr {
                            expr_instance: Some(ExprInstance::ESetBody(eset)),
                        }])),

                        ExprInstance::EMapBody(emap) => {
                            let map = ParMapTypeMapper::emap_to_par_map(emap);

                            Ok(Par::default().with_exprs(vec![Expr {
                                expr_instance: Some(ExprInstance::ESetBody(
                                    ParSetTypeMapper::par_set_to_eset(ParSet::new(
                                        map.ps
                                            .into_iter()
                                            .map(|t| {
                                                Par::default().with_exprs(vec![Expr {
                                                    expr_instance: Some(ExprInstance::ETupleBody(
                                                        ETuple {
                                                            ps: vec![t.0, t.1],
                                                            locally_free: Vec::new(),
                                                            connective_used: false,
                                                        },
                                                    )),
                                                }])
                                            })
                                            .collect(),
                                        map.connective_used,
                                        map.locally_free,
                                        map.remainder,
                                    )),
                                )),
                            }]))
                        }

                        ExprInstance::EListBody(elist) => {
                            Ok(Par::default().with_exprs(vec![Expr {
                                expr_instance: Some(ExprInstance::ESetBody(
                                    ParSetTypeMapper::par_set_to_eset(ParSet::new(
                                        elist.ps,
                                        elist.connective_used,
                                        elist.locally_free,
                                        elist.remainder,
                                    )),
                                )),
                            }]))
                        }

                        other => Err(InterpreterError::MethodNotDefined {
                            method: String::from("to_set"),
                            other_type: get_type(other),
                        }),
                    },

                    None => Err(InterpreterError::MethodNotDefined {
                        method: String::from("to_set"),
                        other_type: String::from("None"),
                    }),
                }
            }
        }

        impl<'a> Method for ToSetMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if !args.is_empty() {
                    Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("to_set"),
                        expected: 0,
                        actual: args.len(),
                    })
                } else {
                    let base_expr = self.outer.eval_single_expr(&p, env)?;
                    let result = self.to_set(base_expr)?;
                    Ok(result)
                }
            }
        }

        Box::new(ToSetMethod { outer: self })
    }

    fn to_map_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct ToMapMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> ToMapMethod<'a> {
            fn make_map(
                &self,
                ps: Vec<Par>,
                connective_used: bool,
                locally_free: Vec<u8>,
                remainder: Option<Var>,
            ) -> Result<Par, InterpreterError> {
                let key_pairs: Vec<Option<(Par, Par)>> =
                    ps.into_iter().map(|p| RhoTuple2::unapply(&p)).collect();

                if key_pairs.iter().any(|pair| !pair.is_some()) {
                    Err(InterpreterError::MethodNotDefined {
                        method: String::from("to_map"),
                        other_type: String::from("types except List[(K,V)]"),
                    })
                } else {
                    Ok(new_emap_par(
                        key_pairs
                            .into_iter()
                            .map(|pair| {
                                let (key, value) = pair.unwrap();
                                KeyValuePair {
                                    key: Some(key),
                                    value: Some(value),
                                }
                            })
                            .collect(),
                        locally_free,
                        connective_used,
                        remainder,
                        Vec::new(),
                        false,
                    ))
                }
            }

            fn to_map(&self, base_expr: Expr) -> Result<Par, InterpreterError> {
                match base_expr.expr_instance {
                    Some(expr_instance) => match expr_instance {
                        ExprInstance::EMapBody(emap) => Ok(Par::default().with_exprs(vec![Expr {
                            expr_instance: Some(ExprInstance::EMapBody(emap)),
                        }])),

                        ExprInstance::ESetBody(eset) => {
                            let base = ParSetTypeMapper::eset_to_par_set(eset);
                            self.make_map(
                                base.ps.sorted_pars,
                                base.connective_used,
                                base.locally_free,
                                base.remainder,
                            )
                        }

                        ExprInstance::EListBody(elist) => self.make_map(
                            elist.ps,
                            elist.connective_used,
                            elist.locally_free,
                            elist.remainder,
                        ),

                        other => Err(InterpreterError::MethodNotDefined {
                            method: String::from("to_map"),
                            other_type: get_type(other),
                        }),
                    },

                    None => Err(InterpreterError::MethodNotDefined {
                        method: String::from("to_map"),
                        other_type: String::from("None"),
                    }),
                }
            }
        }

        impl<'a> Method for ToMapMethod<'a> {
            fn apply(
                &self,
                p: Par,
                args: Vec<Par>,
                env: &Env<Par>,
            ) -> Result<Par, InterpreterError> {
                if !args.is_empty() {
                    Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("to_map"),
                        expected: 0,
                        actual: args.len(),
                    })
                } else {
                    let base_expr = self.outer.eval_single_expr(&p, env)?;
                    let result = self.to_map(base_expr)?;
                    Ok(result)
                }
            }
        }

        Box::new(ToMapMethod { outer: self })
    }

    fn to_string_method<'a>(&'a self) -> Box<dyn Method + 'a> {
        struct ToStringMethod<'a> {
            outer: &'a DebruijnInterpreter,
        }

        impl<'a> ToStringMethod<'a> {
            fn to_string(&self, un: &GUnforgeable) -> Result<Par, InterpreterError> {
                let unf_instance =
                    un.unf_instance
                        .as_ref()
                        .ok_or_else(|| InterpreterError::MethodNotDefined {
                            method: String::from("to_string"),
                            other_type: String::from("None"),
                        })?;

                match unf_instance {
                    UnfInstance::GDeployIdBody(deploy_id) => {
                        Ok(Par::default().with_exprs(vec![Expr {
                            expr_instance: Some(ExprInstance::GString(hex::encode(&deploy_id.sig))),
                        }]))
                    }

                    other => Err(InterpreterError::MethodNotDefined {
                        method: String::from("to_string"),
                        other_type: get_unforgeable_type(other),
                    }),
                }
            }
        }

        impl<'a> Method for ToStringMethod<'a> {
            fn apply(&self, p: Par, args: Vec<Par>, _: &Env<Par>) -> Result<Par, InterpreterError> {
                if !args.is_empty() {
                    Err(InterpreterError::MethodArgumentNumberMismatch {
                        method: String::from("to_map"),
                        expected: 0,
                        actual: args.len(),
                    })
                } else {
                    let un = self.outer.eval_single_unforgeable(&p)?;
                    let result = self.to_string(un)?;
                    Ok(result)
                }
            }
        }

        Box::new(ToStringMethod { outer: self })
    }

    fn method_table<'a>(&'a self) -> HashMap<String, Box<dyn Method + 'a>> {
        let mut table = HashMap::new();
        table.insert("nth".to_string(), self.nth_method());
        table.insert("toByteArray".to_string(), self.to_byte_array_method());
        table.insert("hexToBytes".to_string(), self.hex_to_bytes_method());
        table.insert("bytesToHex".to_string(), self.bytes_to_hex_method());
        table.insert("toUtf8Bytes".to_string(), self.to_utf8_bytes_method());
        table.insert("union".to_string(), self.union_method());
        table.insert("diff".to_string(), self.diff_method());
        table.insert("intersection".to_string(), self.intersection_method());
        table.insert("restriction".to_string(), self.restriction_method());
        table.insert("dropHead".to_string(), self.drop_head_method());
        table.insert("run".to_string(), self.run_method());
        // Zipper methods
        table.insert("readZipper".to_string(), self.read_zipper_method());
        table.insert("readZipperAt".to_string(), self.read_zipper_at_method());
        table.insert("writeZipper".to_string(), self.write_zipper_method());
        table.insert("writeZipperAt".to_string(), self.write_zipper_at_method());
        table.insert("descendTo".to_string(), self.descend_to_method());
        table.insert("getLeaf".to_string(), self.get_leaf_method());
        table.insert("getSubtrie".to_string(), self.get_subtrie_method());
        table.insert("setLeaf".to_string(), self.set_leaf_method());
        table.insert("setSubtrie".to_string(), self.set_subtrie_method());
        table.insert("removeLeaf".to_string(), self.remove_leaf_method());
        table.insert("removeBranches".to_string(), self.remove_branches_method());
        table.insert("graft".to_string(), self.graft_method());
        table.insert("joinInto".to_string(), self.join_into_method());
        table.insert("atPath".to_string(), self.at_path_method());
        table.insert("pathExists".to_string(), self.path_exists_method());
        table.insert("createPath".to_string(), self.create_path_method());
        table.insert("prunePath".to_string(), self.prune_path_method());
        table.insert("reset".to_string(), self.reset_method());
        // Advanced navigation methods
        table.insert("ascendOne".to_string(), self.ascend_one_method());
        table.insert("ascend".to_string(), self.ascend_method());
        table.insert("toNextSibling".to_string(), self.to_next_sibling_method());
        table.insert("toPrevSibling".to_string(), self.to_prev_sibling_method());
        table.insert("descendFirst".to_string(), self.descend_first_method());
        table.insert(
            "descendIndexedBranch".to_string(),
            self.descend_indexed_branch_method(),
        );
        table.insert("childCount".to_string(), self.child_count_method());
        table.insert("add".to_string(), self.add_method());
        table.insert("delete".to_string(), self.delete_method());
        table.insert("contains".to_string(), self.contains_method());
        table.insert("get".to_string(), self.get_method());
        table.insert("getOrElse".to_string(), self.get_or_else_method());
        table.insert("set".to_string(), self.set_method());
        table.insert("keys".to_string(), self.keys_method());
        table.insert("size".to_string(), self.size_method());
        table.insert("length".to_string(), self.length_method());
        table.insert("slice".to_string(), self.slice_method());
        table.insert("take".to_string(), self.take_method());
        table.insert("toList".to_string(), self.to_list_method());
        table.insert("toSet".to_string(), self.to_set_method());
        table.insert("toMap".to_string(), self.to_map_method());
        table.insert("toString".to_string(), self.to_string_method());
        table
    }

    // (eval_single_expr moved: now a thin wrapper over eval_drive, above.)

    fn eval_single_unforgeable<'a>(
        &self,
        p: &'a Par,
    ) -> Result<&'a GUnforgeable, InterpreterError> {
        if !p.sends.is_empty()
            || !p.receives.is_empty()
            || !p.news.is_empty()
            || !p.matches.is_empty()
            || !p.exprs.is_empty()
            || !p.bundles.is_empty()
        {
            Err(InterpreterError::ReduceError(String::from(
                "Error: non unforgeable found where unforgeable expected.",
            )))
        } else {
            match p.unforgeables.as_slice() {
                [e] => Ok(e),

                _ => Err(InterpreterError::ReduceError(
                    "Error: Multiple unforgeables given.".to_string(),
                )),
            }
        }
    }

    // (eval_to_i64 moved: now a thin wrapper over eval_drive, above.)

    // (eval_to_bool moved: now a thin wrapper over eval_drive, above.)

    fn update_locally_free_par(&self, mut par: Par) -> Par {
        let mut locally_free = Vec::new();

        locally_free = union(
            locally_free,
            par.sends
                .iter()
                .flat_map(|send| send.locally_free.clone())
                .collect(),
        );

        locally_free = union(
            locally_free,
            par.receives
                .iter()
                .flat_map(|receive| receive.locally_free.clone())
                .collect(),
        );

        locally_free = union(
            locally_free,
            par.news
                .iter()
                .flat_map(|new_proc| new_proc.locally_free.clone())
                .collect(),
        );

        locally_free = union(
            locally_free,
            par.exprs
                .iter()
                .flat_map(|expr| expr_locally_free_ref(expr))
                .collect(),
        );

        locally_free = union(
            locally_free,
            par.matches
                .iter()
                .flat_map(|match_proc| match_proc.locally_free.clone())
                .collect(),
        );

        locally_free = union(
            locally_free,
            par.bundles
                .iter()
                .flat_map(|bundle_proc| bundle_proc.body.clone().unwrap().locally_free.clone())
                .collect(),
        );

        par.locally_free = locally_free;
        par
    }

    fn update_locally_free_elist(&self, mut elist: EList) -> EList {
        elist.locally_free = elist
            .ps
            .iter()
            .map(|p| p.locally_free.clone())
            .fold(Vec::new(), |acc, locally_free| union(acc, locally_free));

        elist
    }

    fn update_locally_free_etuple(&self, mut etuple: ETuple) -> ETuple {
        etuple.locally_free = etuple
            .ps
            .iter()
            .map(|p| p.locally_free.clone())
            .fold(Vec::new(), |acc, locally_free| union(acc, locally_free));

        etuple
    }

    /**
     * Evaluate any top level expressions in @param Par .
     *
     * Public here to be used in tests / Scala code has it as private but still able to use in tests?
     */
    // (eval_expr moved: now a thin wrapper over eval_drive, above.)

    pub fn new(
        space: RhoISpace,
        urn_map: Arc<HashMap<String, Par>>,
        merge_chs: Arc<RwLock<HashMap<Par, MergeType>>>,
        mergeable_tags: Arc<HashMap<Par, MergeType>>,
        cost: RuntimeBudget,
    ) -> Arc<Self> {
        let reducer_cell = Arc::new(std::sync::OnceLock::new());
        let dispatcher = Arc::new(RholangAndScalaDispatcher {
            _dispatch_table: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            reducer: reducer_cell.clone(),
        });

        let metering = MeteredMachine::new(cost.clone());
        let reducer = Arc::new(DebruijnInterpreter {
            space,
            dispatcher: dispatcher.clone(),
            urn_map,
            merge_chs,
            mergeable_tags,
            metering: metering.clone(),
            substitute: Substitute { metering },
            drive: idle_drive_cell(),
        });

        reducer_cell.set(Arc::downgrade(&reducer)).ok().unwrap();
        reducer
    }
}

fn get_type(expr_instance: ExprInstance) -> String {
    match expr_instance {
        ExprInstance::GBool(_) => String::from("bool"),
        ExprInstance::GInt(_) => String::from("int"),
        ExprInstance::GDouble(_) => String::from("float"),
        ExprInstance::GBigInt(_) => String::from("bigint"),
        ExprInstance::GBigRat(_) => String::from("bigrat"),
        ExprInstance::GFixedPoint(_) => String::from("fixedpoint"),
        ExprInstance::GString(_) => String::from("string"),
        ExprInstance::GUri(_) => String::from("uri"),
        ExprInstance::GByteArray(_) => String::from("byte array"),
        ExprInstance::ENotBody(_) => String::from("enot"),
        ExprInstance::ENegBody(_) => String::from("eneg"),
        ExprInstance::EMultBody(_) => String::from("mult"),
        ExprInstance::EDivBody(_) => String::from("div"),
        ExprInstance::EPlusBody(_) => String::from("plus"),
        ExprInstance::EMinusBody(_) => String::from("minus"),
        ExprInstance::ELtBody(_) => String::from("elt"),
        ExprInstance::ELteBody(_) => String::from("elte"),
        ExprInstance::EGtBody(_) => String::from("egt"),
        ExprInstance::EGteBody(_) => String::from("egte"),
        ExprInstance::EEqBody(_) => String::from("eeq"),
        ExprInstance::ENeqBody(_) => String::from("eneq"),
        ExprInstance::EAndBody(_) => String::from("eand"),
        ExprInstance::EOrBody(_) => String::from("eor"),
        ExprInstance::EVarBody(_) => String::from("evar"),
        ExprInstance::EListBody(_) => String::from("list"),
        ExprInstance::ETupleBody(_) => String::from("tuple"),
        ExprInstance::ESetBody(_) => String::from("set"),
        ExprInstance::EMapBody(_) => String::from("map"),
        ExprInstance::EPathmapBody(_) => String::from("pathmap"),
        ExprInstance::EZipperBody(_) => String::from("zipper"),
        ExprInstance::EMethodBody(_) => String::from("emethod"),
        ExprInstance::EMatchesBody(_) => String::from("ematches"),
        ExprInstance::EPercentPercentBody(_) => String::from("epercent percent"),
        ExprInstance::EPlusPlusBody(_) => String::from("plus plus"),
        ExprInstance::EMinusMinusBody(_) => String::from("minus minus"),
        ExprInstance::EModBody(_) => String::from("mod"),
    }
}

fn get_unforgeable_type(inf_instance: &UnfInstance) -> String {
    match inf_instance {
        UnfInstance::GPrivateBody(_) => String::from("PrivateBody"),
        UnfInstance::GDeployIdBody(_) => String::from("DeployId"),
        UnfInstance::GDeployerIdBody(_) => String::from("DeployerId"),
        UnfInstance::GSysAuthTokenBody(_) => String::from("SysAuthToken"),
    }
}

fn par_contains_nan_double(par: &Par) -> bool {
    par.exprs.iter().any(|e| match &e.expr_instance {
        Some(ExprInstance::GDouble(bits)) => f64::from_bits(*bits).is_nan(),
        Some(ExprInstance::EListBody(list)) => list.ps.iter().any(par_contains_nan_double),
        Some(ExprInstance::ETupleBody(tuple)) => tuple.ps.iter().any(par_contains_nan_double),
        Some(ExprInstance::ESetBody(set)) => set.ps.iter().any(par_contains_nan_double),
        Some(ExprInstance::EMapBody(map)) => map.kvs.iter().any(|kv| {
            kv.key.as_ref().is_some_and(par_contains_nan_double)
                || kv.value.as_ref().is_some_and(par_contains_nan_double)
        }),
        _ => false,
    })
}

fn bytes_to_bigint(bytes: &[u8]) -> num_bigint::BigInt {
    if bytes.is_empty() {
        num_bigint::BigInt::from(0)
    } else {
        num_bigint::BigInt::from_signed_bytes_be(bytes)
    }
}

fn bigint_to_bytes(n: &num_bigint::BigInt) -> Vec<u8> {
    use num_traits::Zero;
    if n.is_zero() {
        vec![0]
    } else {
        n.to_signed_bytes_be()
    }
}

fn make_bigint_expr(bytes: Vec<u8>, _op: &str) -> Result<Expr, InterpreterError> {
    Ok(Expr {
        expr_instance: Some(ExprInstance::GBigInt(bytes)),
    })
}

fn make_bigrat_expr(
    rat: models::rhoapi::GBigRational,
    _op: &str,
) -> Result<Expr, InterpreterError> {
    Ok(Expr {
        expr_instance: Some(ExprInstance::GBigRat(rat)),
    })
}

fn make_fixedpoint_expr(
    fp: models::rhoapi::GFixedPoint,
    _op: &str,
) -> Result<Expr, InterpreterError> {
    Ok(Expr {
        expr_instance: Some(ExprInstance::GFixedPoint(fp)),
    })
}

fn is_zero_twos_complement(bytes: &[u8]) -> bool {
    bytes.is_empty() || bytes.iter().all(|&b| b == 0)
}

fn negate_twos_complement(bytes: &[u8]) -> Vec<u8> {
    let n = bytes_to_bigint(bytes);
    bigint_to_bytes(&(-n))
}

fn add_twos_complement(a: &[u8], b: &[u8]) -> Vec<u8> {
    let result = bytes_to_bigint(a) + bytes_to_bigint(b);
    bigint_to_bytes(&result)
}

fn subtract_twos_complement(a: &[u8], b: &[u8]) -> Vec<u8> {
    let result = bytes_to_bigint(a) - bytes_to_bigint(b);
    bigint_to_bytes(&result)
}

fn multiply_twos_complement(a: &[u8], b: &[u8]) -> Vec<u8> {
    let result = bytes_to_bigint(a) * bytes_to_bigint(b);
    bigint_to_bytes(&result)
}

fn divide_twos_complement(a: &[u8], b: &[u8]) -> Vec<u8> {
    let result = bytes_to_bigint(a) / bytes_to_bigint(b);
    bigint_to_bytes(&result)
}

fn modulo_twos_complement(a: &[u8], b: &[u8]) -> Vec<u8> {
    let result = bytes_to_bigint(a) % bytes_to_bigint(b);
    bigint_to_bytes(&result)
}

fn compare_twos_complement_bytes(a: &[u8], b: &[u8]) -> i32 {
    match bytes_to_bigint(a).cmp(&bytes_to_bigint(b)) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

fn bytes_to_bigrat(rat: &models::rhoapi::GBigRational) -> num_rational::BigRational {
    num_rational::BigRational::new(
        bytes_to_bigint(&rat.numerator),
        bytes_to_bigint(&rat.denominator),
    )
}

fn bigrat_to_proto(r: &num_rational::BigRational) -> models::rhoapi::GBigRational {
    models::rhoapi::GBigRational {
        numerator: bigint_to_bytes(r.numer()),
        denominator: bigint_to_bytes(r.denom()),
    }
}

fn compare_big_rationals(
    a: &models::rhoapi::GBigRational,
    b: &models::rhoapi::GBigRational,
) -> i32 {
    match bytes_to_bigrat(a).cmp(&bytes_to_bigrat(b)) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

fn add_big_rationals(
    a: &models::rhoapi::GBigRational,
    b: &models::rhoapi::GBigRational,
) -> models::rhoapi::GBigRational {
    bigrat_to_proto(&(bytes_to_bigrat(a) + bytes_to_bigrat(b)))
}

fn subtract_big_rationals(
    a: &models::rhoapi::GBigRational,
    b: &models::rhoapi::GBigRational,
) -> models::rhoapi::GBigRational {
    bigrat_to_proto(&(bytes_to_bigrat(a) - bytes_to_bigrat(b)))
}

fn multiply_big_rationals(
    a: &models::rhoapi::GBigRational,
    b: &models::rhoapi::GBigRational,
) -> models::rhoapi::GBigRational {
    bigrat_to_proto(&(bytes_to_bigrat(a) * bytes_to_bigrat(b)))
}

fn divide_big_rationals(
    a: &models::rhoapi::GBigRational,
    b: &models::rhoapi::GBigRational,
) -> models::rhoapi::GBigRational {
    bigrat_to_proto(&(bytes_to_bigrat(a) / bytes_to_bigrat(b)))
}

fn compare_fixed_points(
    a: &models::rhoapi::GFixedPoint,
    b: &models::rhoapi::GFixedPoint,
) -> Result<i32, InterpreterError> {
    if a.scale != b.scale {
        return Err(InterpreterError::OperatorExpectedError {
            op: "cmp".to_string(),
            expected: format!("FixedPoint(p{})", a.scale),
            other_type: format!("FixedPoint(p{})", b.scale),
        });
    }
    Ok(compare_twos_complement_bytes(&a.unscaled, &b.unscaled))
}

fn multiply_fixed_points(
    a: &models::rhoapi::GFixedPoint,
    b: &models::rhoapi::GFixedPoint,
) -> models::rhoapi::GFixedPoint {
    debug_assert_eq!(
        a.scale, b.scale,
        "multiply_fixed_points called with mismatched scales"
    );
    // Scale-preserving: (ua * ub) / 10^scale, using floor division
    let ua = bytes_to_bigint(&a.unscaled);
    let ub = bytes_to_bigint(&b.unscaled);
    let raw = &ua * &ub;
    let ten = num_bigint::BigInt::from(10);
    let scale_factor = num_traits::pow::pow(ten, a.scale as usize);
    let one = num_bigint::BigInt::from(1);
    let unscaled = if raw < num_bigint::BigInt::from(0) {
        // Floor division for negative values
        let abs_raw = -&raw;
        -((&abs_raw - &one) / &scale_factor + &one)
    } else {
        &raw / &scale_factor
    };
    models::rhoapi::GFixedPoint {
        unscaled: bigint_to_bytes(&unscaled),
        scale: a.scale,
    }
}

fn divide_fixed_points(
    a: &models::rhoapi::GFixedPoint,
    b: &models::rhoapi::GFixedPoint,
) -> models::rhoapi::GFixedPoint {
    debug_assert_eq!(
        a.scale, b.scale,
        "divide_fixed_points called with mismatched scales"
    );
    let ten = num_bigint::BigInt::from(10);
    let factor = num_traits::pow::pow(ten, b.scale as usize);
    let scaled = bytes_to_bigint(&a.unscaled) * factor;
    let result = scaled / bytes_to_bigint(&b.unscaled);
    models::rhoapi::GFixedPoint {
        unscaled: bigint_to_bytes(&result),
        scale: a.scale,
    }
}

/// Returns `Some(b)` iff `par` represents the bool value `b` —
/// exactly one Expr (a `GBool`) and nothing else of substance.
fn extract_bool(par: &Par) -> Option<bool> {
    if !par.sends.is_empty()
        || !par.receives.is_empty()
        || !par.news.is_empty()
        || !par.matches.is_empty()
        || !par.bundles.is_empty()
        || !par.unforgeables.is_empty()
        || !par.connectives.is_empty()
        || !par.conditionals.is_empty()
        || par.exprs.len() != 1
    {
        return None;
    }
    match par.exprs[0].expr_instance.as_ref()? {
        ExprInstance::GBool(b) => Some(*b),
        _ => None,
    }
}

fn describe_par_type(par: &Par) -> String {
    if par.exprs.len() == 1
        && par.sends.is_empty()
        && par.receives.is_empty()
        && par.news.is_empty()
        && par.matches.is_empty()
        && par.bundles.is_empty()
        && par.unforgeables.is_empty()
        && par.connectives.is_empty()
        && par.conditionals.is_empty()
    {
        match par.exprs[0].expr_instance.as_ref() {
            Some(ExprInstance::GBool(_)) => "Bool".to_string(),
            Some(ExprInstance::GInt(_)) => "Int".to_string(),
            Some(ExprInstance::GBigInt(_)) => "BigInt".to_string(),
            Some(ExprInstance::GString(_)) => "String".to_string(),
            Some(ExprInstance::GUri(_)) => "Uri".to_string(),
            Some(ExprInstance::GByteArray(_)) => "ByteArray".to_string(),
            Some(ExprInstance::EListBody(_)) => "List".to_string(),
            Some(ExprInstance::ETupleBody(_)) => "Tuple".to_string(),
            Some(ExprInstance::ESetBody(_)) => "Set".to_string(),
            Some(ExprInstance::EMapBody(_)) => "Map".to_string(),
            Some(ExprInstance::EVarBody(_)) => "unbound variable".to_string(),
            _ => "non-boolean expression".to_string(),
        }
    } else {
        "non-boolean process".to_string()
    }
}

// ===========================================================================
// LEG-2 DIFFERENTIAL HARNESS — the correctness proof for the trampoline.
//
// For every term it evaluates the SAME term through BOTH the recursive oracle
// (`eval_expr_recursive`, a faithful copy of the pre-trampoline evaluator over
// the shared `combine_*` helpers) AND the production trampoline (`eval_expr`),
// each on a FRESH budget, and asserts:
//   (1) byte-identical result  (protobuf `encode_to_vec`, or identical Err), AND
//   (2) identical charge trace  (the ordered `(BillableKind, weight)` sequence
//       from the budget's canonical event log — "same tokens, same order, same
//       amounts"), AND
//   (3) identical aggregate `total_cost`.
// Any divergence is a trampoline bug (or a genuine can't-fold fork -> STOP).
// ===========================================================================
#[cfg(test)]
mod differential_trampoline {
    use super::*;
    use crate::rust::interpreter::accounting::BillableKind;
    use crate::rust::interpreter::env::Env;
    use crate::rust::interpreter::test_utils::persistent_store_tester::create_test_space;
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::{
        BindPattern, EAnd, EDiv, EEq, EList, EMatches, EMinus, EMod, EMult, ENeg, ENeq, ENot, EOr,
        EPlus, ETuple, Expr, ListParWithRandom, Par, TaggedContinuation,
    };
    use models::rust::utils::{new_gbool_par, new_gint_par, new_gstring_par};
    use proptest::prelude::*;
    use rspace_plus_plus::rspace::rspace::RSpace;

    type TestSpace = RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

    fn expr_par(ei: ExprInstance) -> Par {
        Par { exprs: vec![Expr { expr_instance: Some(ei) }], ..Default::default() }
    }
    fn eplus(a: Par, b: Par) -> Par { expr_par(ExprInstance::EPlusBody(EPlus { p1: Some(a), p2: Some(b) })) }
    fn eminus(a: Par, b: Par) -> Par { expr_par(ExprInstance::EMinusBody(EMinus { p1: Some(a), p2: Some(b) })) }
    fn emult(a: Par, b: Par) -> Par { expr_par(ExprInstance::EMultBody(EMult { p1: Some(a), p2: Some(b) })) }
    fn ediv(a: Par, b: Par) -> Par { expr_par(ExprInstance::EDivBody(EDiv { p1: Some(a), p2: Some(b) })) }
    fn emod(a: Par, b: Par) -> Par { expr_par(ExprInstance::EModBody(EMod { p1: Some(a), p2: Some(b) })) }
    fn eneg(a: Par) -> Par { expr_par(ExprInstance::ENegBody(ENeg { p: Some(a) })) }
    fn enot(a: Par) -> Par { expr_par(ExprInstance::ENotBody(ENot { p: Some(a) })) }
    fn eand(a: Par, b: Par) -> Par { expr_par(ExprInstance::EAndBody(EAnd { p1: Some(a), p2: Some(b) })) }
    fn eor(a: Par, b: Par) -> Par { expr_par(ExprInstance::EOrBody(EOr { p1: Some(a), p2: Some(b) })) }
    fn eeq(a: Par, b: Par) -> Par { expr_par(ExprInstance::EEqBody(EEq { p1: Some(a), p2: Some(b) })) }
    fn eneq(a: Par, b: Par) -> Par { expr_par(ExprInstance::ENeqBody(ENeq { p1: Some(a), p2: Some(b) })) }
    fn ematches(t: Par, p: Par) -> Par {
        expr_par(ExprInstance::EMatchesBody(EMatches { target: Some(t), pattern: Some(p) }))
    }
    fn elist(ps: Vec<Par>) -> Par {
        expr_par(ExprInstance::EListBody(EList { ps, locally_free: vec![], connective_used: false, remainder: None }))
    }
    fn etuple(ps: Vec<Par>) -> Par {
        expr_par(ExprInstance::ETupleBody(ETuple { ps, locally_free: vec![], connective_used: false }))
    }

    /// The observable trace of one evaluation path: result bytes (or Err string)
    /// + the ordered charge tokens + the aggregate cost.
    #[derive(PartialEq, Debug)]
    struct Trace {
        result: Result<Vec<u8>, String>,
        charges: Vec<(BillableKind, u64)>,
        total: i64,
    }

    fn charge_trace(reducer: &DebruijnInterpreter) -> (Vec<(BillableKind, u64)>, i64) {
        let budget = reducer.metering.budget();
        let charges = budget
            .get_canonical_event_log()
            .into_iter()
            .map(|e| (e.kind, e.weight))
            .collect();
        (charges, budget.total_cost().value)
    }

    async fn build() -> std::sync::Arc<DebruijnInterpreter> {
        let (_space, reducer) = create_test_space::<TestSpace>().await;
        reducer
    }

    /// Evaluate `term` through BOTH paths (fresh reducer/budget each) and assert
    /// byte-identical result + identical charge trace + identical total cost.
    async fn assert_agree(term: &Par) {
        let env: Env<Par> = Env::new();

        let r_rec = build().await;
        let res_rec = r_rec.eval_expr_recursive(term, &env);
        let (charges_rec, total_rec) = charge_trace(&r_rec);
        let trace_rec = Trace {
            result: res_rec.map(|p| p.encode_to_vec()).map_err(|e| format!("{:?}", e)),
            charges: charges_rec,
            total: total_rec,
        };

        let r_tr = build().await;
        let res_tr = r_tr.eval_expr(term, &env);
        let (charges_tr, total_tr) = charge_trace(&r_tr);
        let trace_tr = Trace {
            result: res_tr.map(|p| p.encode_to_vec()).map_err(|e| format!("{:?}", e)),
            charges: charges_tr,
            total: total_tr,
        };

        assert_eq!(
            trace_rec, trace_tr,
            "TRAMPOLINE DIVERGENCE on term {:?}\n recursive={:?}\n trampoline={:?}",
            term, trace_rec, trace_tr
        );
    }

    // ---- hand-written arm coverage (incl. error paths) ----
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn arms_cover_all() {
        let i = |n| new_gint_par(n, vec![], false);
        let b = |x| new_gbool_par(x, vec![], false);
        let s = |x: &str| new_gstring_par(x.to_string(), vec![], false);
        let terms: Vec<Par> = vec![
            i(7),
            b(true),
            s("hi"),
            eplus(i(7), i(8)),
            eminus(i(10), i(3)),
            emult(i(6), i(7)),
            ediv(i(15), i(3)),
            emod(i(17), i(5)),
            ediv(i(1), i(0)),                 // division by zero (error parity)
            emod(i(1), i(0)),                 // modulo by zero
            eplus(i(i64::MAX), i(1)),         // wrapping add
            emult(i(i64::MAX), i(2)),         // multiplication overflow (error parity)
            eneg(i(5)),
            eneg(i(i64::MIN)),                // negation overflow (error parity)
            enot(b(true)),
            eand(b(true), b(false)),
            eor(b(false), b(true)),
            expr_par(ExprInstance::ELtBody(models::rhoapi::ELt { p1: Some(i(1)), p2: Some(i(2)) })),
            expr_par(ExprInstance::EGteBody(models::rhoapi::EGte { p1: Some(i(2)), p2: Some(i(2)) })),
            eeq(i(3), i(3)),
            eneq(i(3), i(4)),
            eeq(elist(vec![i(1), i(2)]), elist(vec![i(1), i(2)])),
            ematches(i(5), i(5)),
            eplus(s("a"), s("b")),            // type error (+ on strings): error parity
            elist(vec![i(1), eplus(i(2), i(3)), i(4)]),
            etuple(vec![b(true), i(9)]),
            // nested arithmetic spine
            eplus(eplus(eplus(i(1), i(2)), i(3)), i(4)),
            // nested collections
            elist(vec![elist(vec![elist(vec![i(0)])])]),
            // mixed
            eand(expr_par(ExprInstance::ELtBody(models::rhoapi::ELt { p1: Some(i(1)), p2: Some(i(2)) })), enot(b(false))),
        ];
        for t in &terms {
            assert_agree(t).await;
        }
    }

    // ---- the six wrappers each vs their _recursive twin (also exercises the
    //      eval_expr_to_expr / eval_single_expr / eval_to_bool / eval_to_i64
    //      wrappers directly) ----
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn wrappers_match_twins() {
        let env: Env<Par> = Env::new();
        let i = |n| new_gint_par(n, vec![], false);
        let b = |x| new_gbool_par(x, vec![], false);
        // eval_expr_to_par / eval_expr_to_expr on an Expr
        let e = Expr { expr_instance: Some(ExprInstance::EPlusBody(EPlus { p1: Some(i(2)), p2: Some(i(5)) })) };
        let r1 = build().await;
        let r2 = build().await;
        assert_eq!(
            r1.eval_expr_to_par_recursive(&e, &env).map_err(|x| format!("{:?}", x)).map(|p| p.encode_to_vec()),
            r2.eval_expr_to_par(&e, &env).map_err(|x| format!("{:?}", x)).map(|p| p.encode_to_vec()),
        );
        let r1 = build().await;
        let r2 = build().await;
        assert_eq!(
            r1.eval_expr_to_expr_recursive(&e, &env).map_err(|x| format!("{:?}", x)),
            r2.eval_expr_to_expr(&e, &env).map_err(|x| format!("{:?}", x)),
        );
        // eval_single_expr
        let p = emult(i(3), i(4));
        let r1 = build().await;
        let r2 = build().await;
        assert_eq!(
            r1.eval_single_expr_recursive(&p, &env).map_err(|x| format!("{:?}", x)),
            r2.eval_single_expr(&p, &env).map_err(|x| format!("{:?}", x)),
        );
        // eval_to_i64
        let r1 = build().await;
        let r2 = build().await;
        assert_eq!(r1.eval_to_i64_recursive(&p, &env).ok(), r2.eval_to_i64(&p, &env).ok());
        // eval_to_bool
        let pb = eand(b(true), enot(b(false)));
        let r1 = build().await;
        let r2 = build().await;
        assert_eq!(r1.eval_to_bool_recursive(&pb, &env).ok(), r2.eval_to_bool(&pb, &env).ok());
    }

    // ---- moderate-depth plus/list: the recursive twin (a DEBUG build, big
    //      frames) survives ~60 levels on the 8 MiB test-thread stack, so both
    //      paths run and must AGREE. (The heap-bounded 20000-deep proof is the
    //      separate `so_probe` example, trampoline-only — the twin cannot reach
    //      it, which is precisely the bug leg-2 fixes.) `current_thread` keeps
    //      the sync recursion on the RUST_MIN_STACK=8 MiB test thread rather
    //      than a 2 MiB tokio worker. ----
    #[tokio::test(flavor = "current_thread")]
    async fn moderate_depth_plus_and_list_agree() {
        let mut t = new_gint_par(0, vec![], false);
        for _ in 0..60 {
            t = eplus(t, new_gint_par(1, vec![], false));
        }
        assert_agree(&t).await;
        let mut u = new_gint_par(0, vec![], false);
        for _ in 0..60 {
            u = elist(vec![u]);
        }
        assert_agree(&u).await;
    }

    // ---- proptest: arbitrary bounded expression trees ----
    fn arb_par() -> impl Strategy<Value = Par> {
        let leaf = prop_oneof![
            (-20i64..20i64).prop_map(|n| new_gint_par(n, vec![], false)),
            any::<bool>().prop_map(|x| new_gbool_par(x, vec![], false)),
            "[a-c]{0,3}".prop_map(|s| new_gstring_par(s, vec![], false)),
        ];
        leaf.prop_recursive(5, 96, 3, |inner| {
            prop_oneof![
                (inner.clone(), inner.clone()).prop_map(|(a, b)| eplus(a, b)),
                (inner.clone(), inner.clone()).prop_map(|(a, b)| eminus(a, b)),
                (inner.clone(), inner.clone()).prop_map(|(a, b)| emult(a, b)),
                (inner.clone(), inner.clone()).prop_map(|(a, b)| ediv(a, b)),
                (inner.clone(), inner.clone()).prop_map(|(a, b)| emod(a, b)),
                inner.clone().prop_map(eneg),
                inner.clone().prop_map(enot),
                (inner.clone(), inner.clone()).prop_map(|(a, b)| eand(a, b)),
                (inner.clone(), inner.clone()).prop_map(|(a, b)| eor(a, b)),
                (inner.clone(), inner.clone()).prop_map(|(a, b)| eeq(a, b)),
                (inner.clone(), inner.clone()).prop_map(|(a, b)| eneq(a, b)),
                (inner.clone(), inner.clone()).prop_map(|(a, b)| {
                    expr_par(ExprInstance::ELtBody(models::rhoapi::ELt { p1: Some(a), p2: Some(b) }))
                }),
                (inner.clone(), inner.clone()).prop_map(|(a, b)| ematches(a, b)),
                prop::collection::vec(inner.clone(), 0..3).prop_map(elist),
                prop::collection::vec(inner.clone(), 0..3).prop_map(etuple),
            ]
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 400, max_shrink_iters: 200, ..ProptestConfig::default() })]
        #[test]
        fn arbitrary_terms_agree(term in arb_par()) {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            rt.block_on(assert_agree(&term));
        }
    }

    // ---- re-eval arms (%% / ++ / -- / Set / Map / method), bound vars, big
    //      numbers, and the remaining ground leaves — the arms whose combine
    //      logic uses re-eval closures / owned-intermediate handling and that the
    //      base coverage above omits. ----
    fn eset(ps: Vec<Par>) -> Par {
        expr_par(ExprInstance::ESetBody(
            models::rust::par_set_type_mapper::ParSetTypeMapper::par_set_to_eset(
                models::rust::par_set::ParSet::create_from_vec(ps),
            ),
        ))
    }
    fn emap(kv: Vec<(Par, Par)>) -> Par {
        expr_par(ExprInstance::EMapBody(
            models::rust::par_map_type_mapper::ParMapTypeMapper::par_map_to_emap(
                models::rust::par_map::ParMap::create_from_vec(kv),
            ),
        ))
    }
    fn method(target: Par, name: &str, args: Vec<Par>) -> Par {
        expr_par(ExprInstance::EMethodBody(models::rhoapi::EMethod {
            method_name: name.to_string(),
            target: Some(target),
            arguments: args,
            locally_free: vec![],
            connective_used: false,
        }))
    }
    fn pplus(a: Par, b: Par) -> Par {
        expr_par(ExprInstance::EPlusPlusBody(models::rhoapi::EPlusPlus { p1: Some(a), p2: Some(b) }))
    }
    fn pmod(a: Par, b: Par) -> Par {
        expr_par(ExprInstance::EPercentPercentBody(models::rhoapi::EPercentPercent {
            p1: Some(a),
            p2: Some(b),
        }))
    }
    fn mminus(a: Par, b: Par) -> Par {
        expr_par(ExprInstance::EMinusMinusBody(models::rhoapi::EMinusMinus {
            p1: Some(a),
            p2: Some(b),
        }))
    }
    fn gbigint(bytes: Vec<u8>) -> Par {
        expr_par(ExprInstance::GBigInt(bytes))
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reeval_collection_and_ground_arms_agree() {
        let i = |n| new_gint_par(n, vec![], false);
        let s = |x: &str| new_gstring_par(x.to_string(), vec![], false);
        let terms: Vec<Par> = vec![
            // ground leaves not covered above
            new_gbool_par(false, vec![], false),
            expr_par(ExprInstance::GUri("rho:io:stdout".to_string())),
            expr_par(ExprInstance::GByteArray(vec![1, 2, 3])),
            expr_par(ExprInstance::GDouble(1.5f64.to_bits())),
            gbigint(vec![7]),
            // Set / Map literals (SORTED owned element eval)
            eset(vec![i(3), i(1), i(2)]),
            eset(vec![eplus(i(1), i(2)), i(5)]),
            emap(vec![(s("a"), i(1)), (s("b"), eplus(i(2), i(3)))]),
            // nested Set / Map
            eset(vec![eset(vec![i(1)]), eset(vec![i(2)])]),
            // ++ (append): string, list, map union, set union
            pplus(s("foo"), s("bar")),
            pplus(elist(vec![i(1)]), elist(vec![i(2), i(3)])),
            pplus(emap(vec![(s("a"), i(1))]), emap(vec![(s("b"), i(2))])),
            pplus(eset(vec![i(1)]), eset(vec![i(2)])),
            // -- (remove): set diff
            mminus(eset(vec![i(1), i(2), i(3)]), eset(vec![i(2)])),
            // %% (interpolation)
            pmod(s("x=${a}"), emap(vec![(s("a"), i(42))])),
            // + set-add sub-arm (Set + elem) and - map/set delete sub-arms
            eplus(eset(vec![i(1)]), i(2)),
            eminus(emap(vec![(s("a"), i(1)), (s("b"), i(2))]), s("a")),
            eminus(eset(vec![i(1), i(2)]), i(2)),
            // methods (EMethod combine: apply + re-eval result)
            method(elist(vec![i(10), i(20), i(30)]), "nth", vec![i(1)]),
            method(elist(vec![i(1), i(2), i(3)]), "length", vec![]),
            method(i(255), "toByteArray", vec![]),
            method(eset(vec![i(1), i(2)]), "toList", vec![]),
            // method chain (target-chain descent + per-link re-eval)
            method(method(elist(vec![elist(vec![i(9)])]), "nth", vec![i(0)]), "nth", vec![i(0)]),
            // type-error method (error parity)
            method(i(5), "nth", vec![i(0)]),
        ];
        for t in &terms {
            assert_agree(t).await;
        }
    }

    // ---- bound variables (EVar arm: eval_var charge + re-eval) under a
    //      POPULATED environment, on both eval_expr and eval_to_i64/bool. ----
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn bound_var_arms_agree() {
        use models::rust::utils::new_boundvar_par;
        // de Bruijn: get(0) returns the LAST `put`. We want BoundVar(0)->41 (int)
        // and BoundVar(1)->[1,2] (list), so put the list FIRST, then the int.
        let mut env: Env<Par> = Env::new();
        env = env.put(elist(vec![new_gint_par(1, vec![], false), new_gint_par(2, vec![], false)]));
        env = env.put(new_gint_par(41, vec![], false));
        let terms: Vec<Par> = vec![
            new_boundvar_par(0, vec![], false),
            new_boundvar_par(1, vec![], false),
            eplus(new_boundvar_par(0, vec![], false), new_gint_par(1, vec![], false)),
            method(new_boundvar_par(1, vec![], false), "nth", vec![new_gint_par(0, vec![], false)]),
        ];
        for t in &terms {
            let r_rec = build().await;
            let res_rec = r_rec.eval_expr_recursive(t, &env);
            let (c_rec, tot_rec) = charge_trace(&r_rec);
            let r_tr = build().await;
            let res_tr = r_tr.eval_expr(t, &env);
            let (c_tr, tot_tr) = charge_trace(&r_tr);
            assert_eq!(
                (res_rec.map(|p| p.encode_to_vec()).map_err(|e| format!("{:?}", e)), c_rec, tot_rec),
                (res_tr.map(|p| p.encode_to_vec()).map_err(|e| format!("{:?}", e)), c_tr, tot_tr),
                "BOUND-VAR DIVERGENCE on {:?}", t
            );
        }
    }

}
