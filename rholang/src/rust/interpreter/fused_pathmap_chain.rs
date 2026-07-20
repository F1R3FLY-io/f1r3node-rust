//! EPathMap fix P2 — T3b: interpreter-internal method-chain view fusion.
//!
//! One seam, [`DebruijnInterpreter::try_eval_fused_method_chain`], called
//! FIRST in BOTH `EMethodBody` dispatch arms (`eval_expr_to_par` and
//! `eval_expr_to_expr`, reduce.rs). It recognizes a read-only PathMap/zipper
//! method chain as one nested AST — the chain is fully visible pre-evaluation
//! because the outermost `EMethod.target` holds the next link — and evaluates
//! the WHOLE chain against a single interned trie plus a lightweight focus
//! (`Vec<Vec<u8>>` mirroring `EZipper.current_path`), instead of today's
//! per-link pipeline that re-evaluates the ground map, converts it to a trie,
//! and materializes an intermediate `EZipper` Par at every link.
//!
//! `Ok(None)` means "not a fusable chain" and the existing per-link path runs
//! UNCHANGED — the seam never partially evaluates before declining (the
//! recognizer performs no charge, no evaluation, and no observable work).
//!
//! # Control neutrality (amendment PM-5(1))
//!
//! The FIRST action is an O(1) method-name check ([`LinkKind::from_name`]):
//! a non-PathMap method (`nth`, `length`, `toString`, …) pays one string
//! compare and nothing else — no spine walk, no env lookup, no interning.
//!
//! # What fuses (the recognizer's shape inventory)
//!
//! * Links (read-only, EXACT arities; write methods never fuse):
//!   - view-preserving (any position): `readZipper`/0, `readZipperAt`/1,
//!     `descendTo`/1, `descendFirst`/0, `descendIndexedBranch`/1, `ascend`/1,
//!     `ascendOne`/0, `toNextSibling`/0, `toPrevSibling`/0, `reset`/0;
//!   - value-producing (TERMINAL only — they exit the view domain):
//!     `pathExists`/0, `getLeaf`/0, `getSubtrie`/0, `childCount`/0,
//!     `atPath`/1. A value-producer mid-chain ⇒ `None` (today's path handles
//!     the follow-on on the produced value).
//! * Spine: every intermediate `target` is a pure single-expr Par whose expr
//!   is the next `EMethodBody` (the same 6-field emptiness `eval_single_expr`
//!   enforces at reduce.rs:7008-7013; `connectives` deliberately unchecked —
//!   today's path ignores them too, and a link result is always rebuilt
//!   fresh so wrapper-Par metadata never propagates).
//! * Base: a single-expr `EVarBody` over a `BoundVar`, resolved BY BORROW via
//!   the additive `Env::get_ref` (rho-pure-eval) to a pure single-expr
//!   `EPathmapBody`/`EZipperBody` Par — or such a literal directly. Anything
//!   else (free/wildcard vars, unbound levels, non-map bindings, `None`
//!   targets, junk-carrying Pars) ⇒ `None`, and the fallback reproduces
//!   today's behavior for those shapes by construction.
//! * GATE (risk R6): `interned_epathmap(map).eval_stable == true`, else
//!   `None`. Today's path re-evaluates the ground map on every var reference
//!   AND at every link's `eval_single_expr` (reduce.rs:2687-2707) — a
//!   re-evaluation that FORCES `remainder = None` and recomputes
//!   `locally_free` per entry. The PM-4(c) classifier admits exactly the maps
//!   for which that pipeline is the byte-exact identity, so skipping it is
//!   byte-invisible; ground re-evaluation adds NO charges (verified: no
//!   `reserve_*` in the EPathmapBody/ground arms :2687-2714), so skipping it
//!   is charge-invisible too. Conservative: anything unrecognized falls back.
//!
//! # Charge replay (amendment PM-4(b) — same entry points, same constants,
//! same ORDER)
//!
//! The fused replay issues the SAME `MeteredMachine` reservations today's
//! path issues, in the same within-fork temporal order (local_index order —
//! which is what the canonical event log's `Ord` keys on):
//!
//! 1. `reserve_primitive(method_call_cost())` × n, outermost-first — today
//!    each dispatch charges at reduce.rs:1536/:2723 BEFORE recursing into its
//!    target, so the outermost link's charge lands first.
//! 2. `reserve_primitive(var_eval_cost())` for a var base (:1217). Literal
//!    bases charge nothing (:1556's ground fall-through is charge-free).
//! 3. Per link, innermost→outermost, mirroring each `Method::apply`:
//!    a. Position-A argument evaluation (:1538-1542) — the SAME
//!       `self.eval_expr(arg, env)` on the raw argument ASTs, so var/method
//!       arguments charge exactly as today, at today's position;
//!    b. the arity check (:3693-3698 et al.) — recognizer-guaranteed exact,
//!       so in-fusion it cannot fire; wrong-arity chains never fuse and the
//!       fallback raises the identical `MethodArgumentNumberMismatch`;
//!    c. the `apply`-entry target check (`eval_single_expr` :7007-7027): a
//!       Nil view raises the exact `_`-arm
//!       `ReduceError("Error: Multiple expressions given.")` — the PM-4(d)
//!       parity target — with the failing link's arguments already charged
//!       and its constant NOT charged;
//!    d. Position-B argument re-evaluation for arity-1 links
//!       (:3701/:3886/:4973/:5402/:5667) — replayed verbatim (charge-free on
//!       already-evaluated input, but the VALUE pipeline is preserved
//!       byte-for-byte rather than proven idempotent);
//!    e. the link constant: `reserve_incremental_primitive(union_cost(1))`
//!       for the 13 navigation links (:3625/:3704/:3889/:4976/:5051/:5271/
//!       :5327/:5405/:5484/:5564/:5670/:5760/:5850),
//!       `reserve_primitive(lookup_cost())` for `getLeaf`/`getSubtrie`
//!       (:3975/:4054);
//!    f. the link semantics on the view (see below) — including
//!       `MethodNotDefined` when the view mode does not match the link's
//!       accepted arms, and `ascend`/`descendIndexedBranch`'s argument
//!       extraction errors, which today fire AFTER the union constant.
//!
//! Because primitives carry zero consensus cost units (D3: only `Comm`
//! charges gate liveness), mid-sequence budget exhaustion behavior is
//! preserved trivially — and the replay preserves it structurally anyway by
//! issuing byte-identical reservation sequences.
//!
//! # Link semantics (pinned per the landed reduce.rs impls)
//!
//! The view state is `(Arc<InternedEPathMap>, focus: Vec<Vec<u8>>)` plus the
//! zipper metadata a materialization needs. Keys are the 0xFF-terminated
//! segment flattening (`seg ∥ 0xFF ∥ …`, reduce.rs:3998-4007;
//! `SEGMENT_SEPARATOR`, models pathmap_native_query.rs:32). Each fused link
//! mirrors its today-impl arm-for-arm — the same
//! `collect_child_segments`/`collect_subtrie_values`/`path_prefix_exists`
//! helpers on the same (now shared, uncloned) trie, the same
//! `RholangReadZipper::new(…).get_val()` for `getLeaf`'s raw-map root
//! variant (:3938-3950), the same `pathExists` empty-focus special case
//! (:5011-5013), the same message-level `ps.is_empty()` reads, the same Nil
//! productions (getLeaf-no-value :3929, descendFirst-no-children :5536,
//! ascendOne-at-root :5292, descendIndexedBranch-negative :5598, sibling
//! misses :5728/:5732/:5818/:5822), and the same `MethodNotDefined` payloads
//! (`get_type` yields `"pathmap"`/`"zipper"` for the two view-mode exprs,
//! reduce.rs:7312-7313).
//!
//! Zipper-terminal chains materialize the EXACT `EZipper` Par today's path
//! produces — one map-message embed cloned from the borrowed base (parity
//! only, no win claimed on that arm).
//!
//! # Force-disable + instrumentation (amendment PM-5(3))
//!
//! [`fusion_test_support`] — the differential harness's force-disable toggle
//! and the shape-keyed fusion-hit counters — is compiled ONLY under
//! `cfg(test)` or the `epathmap-fusion-differential` feature. Production
//! builds contain NO runtime-flippable fusion path (a runtime flag would be
//! a node-divergence hazard under a latent parity bug).

use std::sync::Arc;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{EMethod, EPathMap, EZipper, Expr, Par};
use models::rust::pathmap_crate_type_mapper::{interned_epathmap, InternedEPathMap};
use models::rust::pathmap_integration::par_to_path;
use models::rust::pathmap_native_query::{
    collect_child_segments, collect_subtrie_values, path_prefix_exists, SEGMENT_SEPARATOR,
};
use models::rust::pathmap_zipper::RholangReadZipper;

use super::accounting::costs::{lookup_cost, method_call_cost, union_cost, var_eval_cost};
use super::env::Env;
use super::errors::InterpreterError;
use super::reduce::DebruijnInterpreter;

/// `get_type(ExprInstance::EPathmapBody(_))` (reduce.rs:7312) — the
/// `MethodNotDefined::other_type` payload for a map-mode view.
const TYPE_PATHMAP: &str = "pathmap";

/// `get_type(ExprInstance::EZipperBody(_))` (reduce.rs:7313) — the
/// `MethodNotDefined::other_type` payload for a zipper-mode view.
const TYPE_ZIPPER: &str = "zipper";

/// The exact `eval_single_expr` `_`-arm message (reduce.rs:7022-7024) — the
/// PM-4(d) Nil-mid-chain parity target (the misleading string IS the pin).
const NIL_MID_CHAIN_ERROR: &str = "Error: Multiple expressions given.";

// ─────────────────────────────────────────────────────────────────────────────
// The fusable-link inventory
// ─────────────────────────────────────────────────────────────────────────────

/// The 15 fusable read-only links. Write methods (`writeZipper`,
/// `writeZipperAt`, `setLeaf`, `setSubtrie`, `removeLeaf`, `removeBranches`,
/// `graft`, `joinInto`, `createPath`, `prunePath`) are deliberately absent —
/// they NEVER fuse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LinkKind {
    ReadZipper,
    ReadZipperAt,
    DescendTo,
    DescendFirst,
    DescendIndexedBranch,
    Ascend,
    AscendOne,
    ToNextSibling,
    ToPrevSibling,
    Reset,
    PathExists,
    GetLeaf,
    GetSubtrie,
    ChildCount,
    AtPath,
}

/// Which metering entry point a link's constant goes through (PM-4(b)).
enum LinkCharge {
    /// `reserve_incremental_primitive(union_cost(1))` — the 13 navigation
    /// links.
    IncrementalUnion,
    /// `reserve_primitive(lookup_cost())` — `getLeaf`/`getSubtrie`.
    Lookup,
}

impl LinkKind {
    /// The PM-5(1) name gate: one O(1) string match; `None` for every
    /// non-fusable method name.
    fn from_name(name: &str) -> Option<LinkKind> {
        match name {
            "readZipper" => Some(LinkKind::ReadZipper),
            "readZipperAt" => Some(LinkKind::ReadZipperAt),
            "descendTo" => Some(LinkKind::DescendTo),
            "descendFirst" => Some(LinkKind::DescendFirst),
            "descendIndexedBranch" => Some(LinkKind::DescendIndexedBranch),
            "ascend" => Some(LinkKind::Ascend),
            "ascendOne" => Some(LinkKind::AscendOne),
            "toNextSibling" => Some(LinkKind::ToNextSibling),
            "toPrevSibling" => Some(LinkKind::ToPrevSibling),
            "reset" => Some(LinkKind::Reset),
            "pathExists" => Some(LinkKind::PathExists),
            "getLeaf" => Some(LinkKind::GetLeaf),
            "getSubtrie" => Some(LinkKind::GetSubtrie),
            "childCount" => Some(LinkKind::ChildCount),
            "atPath" => Some(LinkKind::AtPath),
            _ => None,
        }
    }

    /// The method-name string exactly as today's `MethodNotDefined` payloads
    /// spell it (each `Method` impl uses its registered name).
    fn method_name(self) -> &'static str {
        match self {
            LinkKind::ReadZipper => "readZipper",
            LinkKind::ReadZipperAt => "readZipperAt",
            LinkKind::DescendTo => "descendTo",
            LinkKind::DescendFirst => "descendFirst",
            LinkKind::DescendIndexedBranch => "descendIndexedBranch",
            LinkKind::Ascend => "ascend",
            LinkKind::AscendOne => "ascendOne",
            LinkKind::ToNextSibling => "toNextSibling",
            LinkKind::ToPrevSibling => "toPrevSibling",
            LinkKind::Reset => "reset",
            LinkKind::PathExists => "pathExists",
            LinkKind::GetLeaf => "getLeaf",
            LinkKind::GetSubtrie => "getSubtrie",
            LinkKind::ChildCount => "childCount",
            LinkKind::AtPath => "atPath",
        }
    }

    /// The EXACT argument count each `Method::apply` accepts (its arity
    /// check at reduce.rs:3615/:3693/:3878/:3967/:4046/:4965/:5041/:5213/
    /// :5261/:5317/:5394/:5474/:5554/:5659/:5750/:5840). Any other count ⇒
    /// the chain never fuses and the fallback raises the identical
    /// `MethodArgumentNumberMismatch` at the identical point-in-sequence.
    fn exact_arity(self) -> usize {
        match self {
            LinkKind::ReadZipperAt
            | LinkKind::DescendTo
            | LinkKind::DescendIndexedBranch
            | LinkKind::Ascend
            | LinkKind::AtPath => 1,
            LinkKind::ReadZipper
            | LinkKind::DescendFirst
            | LinkKind::AscendOne
            | LinkKind::ToNextSibling
            | LinkKind::ToPrevSibling
            | LinkKind::Reset
            | LinkKind::PathExists
            | LinkKind::GetLeaf
            | LinkKind::GetSubtrie
            | LinkKind::ChildCount => 0,
        }
    }

    /// View-preserving links may appear anywhere in the spine; value
    /// producers only as the TERMINAL (outermost) link.
    fn is_view_preserving(self) -> bool {
        matches!(
            self,
            LinkKind::ReadZipper
                | LinkKind::ReadZipperAt
                | LinkKind::DescendTo
                | LinkKind::DescendFirst
                | LinkKind::DescendIndexedBranch
                | LinkKind::Ascend
                | LinkKind::AscendOne
                | LinkKind::ToNextSibling
                | LinkKind::ToPrevSibling
                | LinkKind::Reset
        )
    }

    /// PM-4(b): which reservation entry point the link constant uses.
    fn charge(self) -> LinkCharge {
        match self {
            LinkKind::GetLeaf | LinkKind::GetSubtrie => LinkCharge::Lookup,
            _ => LinkCharge::IncrementalUnion,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The recognized chain
// ─────────────────────────────────────────────────────────────────────────────

/// How the chain's base was found (drives the `var_eval_cost` replay and the
/// initial view mode).
enum FusedBase<'a> {
    /// `EVarBody` base bound to a single-expr `EPathmapBody` Par — charges
    /// `var_eval_cost`, starts in map mode.
    VarMap,
    /// `EVarBody` base bound to a single-expr `EZipperBody` Par — charges
    /// `var_eval_cost`, starts in zipper mode at the zipper's focus.
    VarZipper(&'a EZipper),
    /// Literal `EPathmapBody` base — no base charge, map mode.
    LitMap,
    /// Literal `EZipperBody` base — no base charge, zipper mode.
    LitZipper(&'a EZipper),
}

/// A fully recognized, GATED fusable chain. Once this exists, the replay's
/// result (value or error) is FINAL — there is no post-charge fallback,
/// because falling back after replay began would double-charge.
struct FusedChain<'a> {
    /// Walk order: `links[0]` = OUTERMOST (applied last, the terminal link);
    /// `links[n-1]` = innermost (applied first).
    links: Vec<&'a EMethod>,
    /// `kinds[i]` classifies `links[i]`.
    kinds: Vec<LinkKind>,
    base: FusedBase<'a>,
    /// The interned conversion of `source_map` — the ONE trie every link
    /// reads (an `Arc` field access per link instead of today's per-link
    /// O(map) digest re-walk through `e_pathmap_to_rholang_pathmap`).
    interned: Arc<InternedEPathMap>,
    /// The source EPathMap MESSAGE (borrowed from the base binding or the
    /// literal). Under the `eval_stable` gate this is byte-identical to
    /// every re-evaluated copy today's path constructs, so it is THE message
    /// to embed when a zipper materializes and THE message `getSubtrie`'s
    /// map-mode arm returns.
    source_map: &'a EPathMap,
}

impl FusedChain<'_> {
    /// Shape key for the test-only fusion-hit counters:
    /// `{base}:{innermost.…​.outermost}` — e.g.
    /// `var-map:readZipperAt.getSubtrie`.
    #[cfg(any(test, feature = "epathmap-fusion-differential"))]
    fn shape_key(&self) -> String {
        let base = match self.base {
            FusedBase::VarMap => "var-map",
            FusedBase::VarZipper(_) => "var-zipper",
            FusedBase::LitMap => "lit-map",
            FusedBase::LitZipper(_) => "lit-zipper",
        };
        let mut key = String::with_capacity(16 + self.kinds.len() * 16);
        key.push_str(base);
        key.push(':');
        for (position, kind) in self.kinds.iter().rev().enumerate() {
            if position > 0 {
                key.push('.');
            }
            key.push_str(kind.method_name());
        }
        key
    }
}

/// The pure single-expr carrier test — the SAME six-field emptiness
/// `eval_single_expr` enforces (reduce.rs:7008-7013). `connectives`,
/// `locally_free`, and `connective_used` are deliberately NOT checked:
/// today's path ignores them at every point a chain Par flows through
/// (`eval_expr` preserves them into the evaluated wrapper, `eval_single_expr`
/// never looks, and every link result is rebuilt fresh), so they cannot
/// affect parity.
fn pure_single_expr(par: &Par) -> Option<&ExprInstance> {
    if !par.sends.is_empty()
        || !par.receives.is_empty()
        || !par.news.is_empty()
        || !par.matches.is_empty()
        || !par.unforgeables.is_empty()
        || !par.bundles.is_empty()
    {
        return None;
    }
    match par.exprs.as_slice() {
        [expr] => expr.expr_instance.as_ref(),
        _ => None,
    }
}

/// Recognize the maximal fusable spine rooted at `emethod` and gate it.
/// `None` ⇒ the caller must run today's per-link path unchanged.
fn recognize_chain<'a>(emethod: &'a EMethod, env: &'a Env<Par>) -> Option<FusedChain<'a>> {
    let mut links: Vec<&'a EMethod> = Vec::new();
    let mut kinds: Vec<LinkKind> = Vec::new();
    let mut cursor: &'a EMethod = emethod;

    let base_par: &'a Par = loop {
        let kind = LinkKind::from_name(&cursor.method_name)?;
        if cursor.arguments.len() != kind.exact_arity() {
            // Wrong arity ⇒ fallback; today's `apply` raises the identical
            // MethodArgumentNumberMismatch after argument evaluation.
            return None;
        }
        if !links.is_empty() && !kind.is_view_preserving() {
            // A value producer below the terminal position exits the view
            // domain (its result is an arbitrary stored Par) — fallback.
            return None;
        }
        links.push(cursor);
        kinds.push(kind);

        let target = cursor.target.as_ref()?;
        match pure_single_expr(target) {
            Some(ExprInstance::EMethodBody(next)) => cursor = next,
            _ => break target,
        }
    };

    let (base, source_map): (FusedBase<'a>, &'a EPathMap) = match pure_single_expr(base_par)? {
        ExprInstance::EPathmapBody(map) => (FusedBase::LitMap, map),
        // A zipper whose embedded map is `None` would make today's methods
        // panic on their `.expect("zipper pathmap was None")` — fall back so
        // the behavior (panic included) stays today's.
        ExprInstance::EZipperBody(zipper) => (FusedBase::LitZipper(zipper), zipper.pathmap.as_ref()?),
        ExprInstance::EVarBody(evar) => {
            let var = evar.v.as_ref()?;
            match &var.var_instance {
                Some(VarInstance::BoundVar(level)) => {
                    // Resolve BY BORROW (the additive Env::get_ref). An
                    // unbound level falls back: today charges var_eval then
                    // raises the "Unbound variable" ReduceError, and the
                    // fallback reproduces exactly that.
                    let bound: &'a Par = env.get_ref(level)?;
                    match pure_single_expr(bound)? {
                        ExprInstance::EPathmapBody(map) => (FusedBase::VarMap, map),
                        ExprInstance::EZipperBody(zipper) => {
                            (FusedBase::VarZipper(zipper), zipper.pathmap.as_ref()?)
                        }
                        _ => return None,
                    }
                }
                // Wildcard/FreeVar/None: today's eval_var raises — fallback.
                _ => return None,
            }
        }
        _ => return None,
    };

    // THE GATE (plan §1-P2, risk R6): fusion skips today's ground
    // re-evaluation, which is only byte-invisible when the PM-4(c)
    // classifier certifies the map as eval-stable. Interning here also
    // pre-warms the one store entry every link will read — the same entry
    // today's first conversion would create (same message bytes ⇒ same
    // digest), so no second cache mechanism is introduced (risk R3).
    let interned = interned_epathmap(source_map);
    if !interned.eval_stable {
        return None;
    }

    Some(FusedChain {
        links,
        kinds,
        base,
        interned,
        source_map,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// The view evaluator
// ─────────────────────────────────────────────────────────────────────────────

/// Zipper metadata carried by navigation links unchanged (today each link
/// clones the whole `EZipper` and mutates only `current_path`; these three
/// fields ride along byte-identically).
struct ZipperMeta {
    is_write: bool,
    locally_free: Vec<u8>,
    connective_used: bool,
}

/// The view a chain link operates on.
enum ViewMode {
    /// The base map itself, at root, no zipper created yet (only reachable
    /// as the initial state — every map-mode link either creates a zipper,
    /// produces a terminal value, or raises `MethodNotDefined`).
    Map,
    /// A read/write zipper view: `focus` mirrors `EZipper.current_path`.
    Zipper { focus: Vec<Vec<u8>>, meta: ZipperMeta },
    /// A mid-chain Nil (`Par::default()`): any follow-on link raises the
    /// exact `eval_single_expr` error; a terminal Nil materializes as `Nil`.
    Nil,
}

/// The 0xFF-terminated key flattening (reduce.rs:3998-4007): every segment
/// contributes `seg ∥ SEGMENT_SEPARATOR`. Preallocated to the exact final
/// length.
fn flatten_key(segments: &[Vec<u8>]) -> Vec<u8> {
    let mut key = Vec::with_capacity(segments.iter().map(|seg| seg.len() + 1).sum());
    for seg in segments {
        key.extend_from_slice(seg);
        key.push(SEGMENT_SEPARATOR);
    }
    key
}

/// `MethodNotDefined` with the exact payloads today's arms construct.
fn method_not_defined(kind: LinkKind, other_type: &str) -> InterpreterError {
    InterpreterError::MethodNotDefined {
        method: String::from(kind.method_name()),
        other_type: other_type.to_string(),
    }
}

/// A single-expr result Par (`Par::default().with_exprs(vec![…])`) — the
/// exact constructor every zipper-method `apply` uses for its result.
fn single_expr_par(expr_instance: ExprInstance) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(expr_instance),
    }])
}

impl DebruijnInterpreter {
    /// THE P2 SEAM. `Ok(None)` ⇒ not fusable, run today's path unchanged;
    /// `Ok(Some(par))` ⇒ the chain's result, byte-identical to today's, with
    /// byte-identical charges already reserved; `Err` ⇒ the chain fused and
    /// failed exactly as today's path would have (same error, same charges).
    pub(crate) fn try_eval_fused_method_chain(
        &self,
        emethod: &EMethod,
        env: &Env<Par>,
    ) -> Result<Option<Par>, InterpreterError> {
        // PM-5(3): the force-disable seam exists ONLY in test builds — no
        // production runtime flag.
        #[cfg(any(test, feature = "epathmap-fusion-differential"))]
        {
            if fusion_test_support::force_disabled() {
                return Ok(None);
            }
        }

        // PM-5(1): O(1) NAME GATE before any spine walk — a non-PathMap
        // method pays exactly one string compare here.
        if LinkKind::from_name(&emethod.method_name).is_none() {
            return Ok(None);
        }

        let chain = match recognize_chain(emethod, env) {
            Some(chain) => chain,
            None => return Ok(None),
        };

        #[cfg(any(test, feature = "epathmap-fusion-differential"))]
        let shape_key = chain.shape_key();

        let result = self.replay_fused_chain(&chain, env);

        // A fused chain that ERRORS still counts as a fusion hit (the fused
        // path owned the evaluation); recorded before propagating.
        #[cfg(any(test, feature = "epathmap-fusion-differential"))]
        fusion_test_support::record_hit(shape_key);

        result.map(Some)
    }

    /// Replay the recognized chain: charges in today's exact order, link
    /// semantics on the shared view. See the module docs for the pinned
    /// order (method_call ×n outermost-first → base var_eval → per link
    /// innermost-out: Position-A args → Nil check → Position-B args → link
    /// constant → semantics).
    fn replay_fused_chain(
        &self,
        chain: &FusedChain<'_>,
        env: &Env<Par>,
    ) -> Result<Par, InterpreterError> {
        // (1) method_call × n, outermost-first (reduce.rs:1536/:2723 charge
        // BEFORE recursing into the target, so today the outermost dispatch
        // charges first and the chain aborting later still leaves all n
        // charges committed — e.g. the pinned NIL_GETLEAF_TRACE opens with
        // three `prim(method call)=10` rows).
        for _ in &chain.links {
            self.metering.reserve_primitive(method_call_cost())?;
        }

        // (2) the base: a var base charges var_eval_cost (:1217); the bound
        // map's re-evaluation adds NO charges under the eval_stable gate
        // (and a bound zipper is returned as-is today, :2710-2714).
        if matches!(chain.base, FusedBase::VarMap | FusedBase::VarZipper(_)) {
            self.metering.reserve_primitive(var_eval_cost())?;
        }

        // (3) the initial view.
        let mut mode = match &chain.base {
            FusedBase::VarMap | FusedBase::LitMap => ViewMode::Map,
            FusedBase::VarZipper(zipper) | FusedBase::LitZipper(zipper) => ViewMode::Zipper {
                focus: zipper.current_path.clone(),
                meta: ZipperMeta {
                    is_write: zipper.is_write_zipper,
                    locally_free: zipper.locally_free.clone(),
                    connective_used: zipper.connective_used,
                },
            },
        };

        // (4) links innermost → outermost. `links[0]` is the OUTERMOST
        // (terminal) link, so iterate indices n-1 … 0; value producers can
        // only sit at index 0 (recognizer invariant) and `return` directly.
        for idx in (0..chain.links.len()).rev() {
            let link = chain.links[idx];
            let kind = chain.kinds[idx];

            // (a) Position-A argument evaluation (:1538-1542): the same
            // eval_expr on the raw argument ASTs — full charges (var_eval
            // for var arguments, method_call for method arguments, …) at
            // today's position: after the inner links completed, before the
            // apply-order steps below.
            let mut args_a: Vec<Par> = Vec::with_capacity(link.arguments.len());
            for arg in &link.arguments {
                args_a.push(self.eval_expr(arg, env)?);
            }

            // (b) the arity check — recognizer-guaranteed exact (wrong-arity
            // chains never fuse; the fallback raises the mismatch).
            debug_assert_eq!(
                args_a.len(),
                kind.exact_arity(),
                "fused link arity must be recognizer-guaranteed"
            );

            // (c) the apply-entry target check (eval_single_expr, called on
            // the evaluated target at :3622/:3700/:3824/:3885/:3974/:4053/
            // :4972/:5048/:5220/:5268/:5324/:5401/:5481/:5561/:5666/:5757/
            // :5847): a Nil target (zero exprs) hits the `_` arm — the exact
            // PM-4(d) error, with this link's arguments already charged and
            // its constant NOT charged. Map/zipper targets pass: the map
            // re-evaluation is the byte-identity under the gate, the zipper
            // arm returns as-is — both charge-free.
            if matches!(mode, ViewMode::Nil) {
                return Err(InterpreterError::ReduceError(NIL_MID_CHAIN_ERROR.to_string()));
            }

            // (d) Position-B argument re-evaluation for arity-1 links
            // (:3701/:3886/:4973/:5402/:5667) — today `apply` re-evaluates
            // the ALREADY-evaluated argument; replayed verbatim so the value
            // pipeline (including locally_free recomputation on list
            // arguments) is byte-identical rather than argued idempotent.
            let arg_b: Option<Par> = if kind.exact_arity() == 1 {
                Some(self.eval_expr(&args_a[0], env)?)
            } else {
                None
            };

            // (e) the link constant (PM-4(b) — same entry point, same Cost).
            match kind.charge() {
                LinkCharge::IncrementalUnion => self
                    .metering
                    .reserve_incremental_primitive(union_cost(1))?,
                LinkCharge::Lookup => self.metering.reserve_primitive(lookup_cost())?,
            }

            // (f) the link semantics on the view.
            match kind {
                // ── readZipper (:3585-3628) ─────────────────────────────
                LinkKind::ReadZipper => match &mode {
                    ViewMode::Map => {
                        // :3589-3595 — fresh read zipper at root; its
                        // locally_free/connective_used are vec![]/false (NOT
                        // copied from the map).
                        mode = ViewMode::Zipper {
                            focus: Vec::new(),
                            meta: ZipperMeta {
                                is_write: false,
                                locally_free: Vec::new(),
                                connective_used: false,
                            },
                        };
                    }
                    ViewMode::Zipper { .. } => {
                        // :3600-3603 — a zipper target is not a pathmap.
                        return Err(method_not_defined(kind, TYPE_ZIPPER));
                    }
                    ViewMode::Nil => unreachable!("Nil views return at step (c)"),
                },

                // ── readZipperAt (:3639-3707) ───────────────────────────
                LinkKind::ReadZipperAt => match &mode {
                    ViewMode::Map => {
                        let path_par =
                            arg_b.as_ref().expect("arity-1 link must have a Position-B argument");
                        // :3650 par_to_path on the Position-B argument;
                        // :3661-3671 — locally_free/connective_used copied
                        // from the map MESSAGE (empty/false under the gate,
                        // but copied for exactness).
                        mode = ViewMode::Zipper {
                            focus: par_to_path(path_par),
                            meta: ZipperMeta {
                                is_write: false,
                                locally_free: chain.source_map.locally_free.clone(),
                                connective_used: chain.source_map.connective_used,
                            },
                        };
                    }
                    ViewMode::Zipper { .. } => {
                        // :3678-3681.
                        return Err(method_not_defined(kind, TYPE_ZIPPER));
                    }
                    ViewMode::Nil => unreachable!("Nil views return at step (c)"),
                },

                // ── descendTo (:3843-3892) ──────────────────────────────
                LinkKind::DescendTo => match &mut mode {
                    ViewMode::Zipper { focus, .. } => {
                        let path_par =
                            arg_b.as_ref().expect("arity-1 link must have a Position-B argument");
                        // :3853-3857 — append WITHOUT existence checking.
                        focus.extend(par_to_path(path_par));
                    }
                    ViewMode::Map => {
                        // :3863-3866 — descendTo has NO EPathmapBody arm.
                        return Err(method_not_defined(kind, TYPE_PATHMAP));
                    }
                    ViewMode::Nil => unreachable!("Nil views return at step (c)"),
                },

                // ── descendFirst (:5502-5544) ───────────────────────────
                LinkKind::DescendFirst => {
                    let transition = match &mut mode {
                        ViewMode::Zipper { focus, .. } => {
                            // :5526 — first (byte-lex smallest) child.
                            let children = collect_child_segments(
                                &chain.interned.map,
                                &flatten_key(focus),
                                Some(1),
                            );
                            match children.into_iter().next() {
                                Some(first) => {
                                    focus.push(first);
                                    None
                                }
                                // :5534-5536 — no children ⇒ Nil.
                                None => Some(ViewMode::Nil),
                            }
                        }
                        ViewMode::Map => {
                            // :5539-5542 — no EPathmapBody arm.
                            return Err(method_not_defined(kind, TYPE_PATHMAP));
                        }
                        ViewMode::Nil => unreachable!("Nil views return at step (c)"),
                    };
                    if let Some(next) = transition {
                        mode = next;
                    }
                }

                // ── descendIndexedBranch (:5578-5648) ───────────────────
                LinkKind::DescendIndexedBranch => {
                    let idx_par =
                        arg_b.as_ref().expect("arity-1 link must have a Position-B argument");
                    // :5584-5594 — the integer extraction precedes the base
                    // match…
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
                    // :5596-5599 — …and so does the negative check: a
                    // negative index yields Nil EVEN on a map-mode view.
                    if idx < 0 {
                        mode = ViewMode::Nil;
                    } else {
                        let transition = match &mut mode {
                            ViewMode::Zipper { focus, .. } => {
                                // :5627-5631 — early-stop after idx+1
                                // emissions (saturating: a saturated limit
                                // enumerates all children and `.get` still
                                // yields None).
                                let children = collect_child_segments(
                                    &chain.interned.map,
                                    &flatten_key(focus),
                                    Some((idx as usize).saturating_add(1)),
                                );
                                match children.into_iter().nth(idx as usize) {
                                    Some(child) => {
                                        focus.push(child);
                                        None
                                    }
                                    // :5639-5641 — out of bounds ⇒ Nil.
                                    None => Some(ViewMode::Nil),
                                }
                            }
                            ViewMode::Map => {
                                // :5644-5647.
                                return Err(method_not_defined(kind, TYPE_PATHMAP));
                            }
                            ViewMode::Nil => unreachable!("Nil views return at step (c)"),
                        };
                        if let Some(next) = transition {
                            mode = next;
                        }
                    }
                }

                // ── ascend (:5341-5410) ─────────────────────────────────
                LinkKind::Ascend => {
                    let steps_par =
                        arg_b.as_ref().expect("arity-1 link must have a Position-B argument");
                    // :5343-5355 — extraction precedes the base match.
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
                    // :5357-5362 — so does the negative check.
                    if steps < 0 {
                        return Err(InterpreterError::MethodNotDefined {
                            method: String::from("ascend (steps must be non-negative)"),
                            other_type: format!("negative: {}", steps),
                        });
                    }
                    match &mut mode {
                        ViewMode::Zipper { focus, .. } => {
                            // :5366-5373 — pop up to `steps`, capped at root.
                            let actual_steps = std::cmp::min(steps as usize, focus.len());
                            for _ in 0..actual_steps {
                                focus.pop();
                            }
                        }
                        ViewMode::Map => {
                            // :5379-5382.
                            return Err(method_not_defined(kind, TYPE_PATHMAP));
                        }
                        ViewMode::Nil => unreachable!("Nil views return at step (c)"),
                    }
                }

                // ── ascendOne (:5286-5307) ──────────────────────────────
                LinkKind::AscendOne => {
                    let transition = match &mut mode {
                        ViewMode::Zipper { focus, .. } => {
                            if focus.is_empty() {
                                // :5290-5293 — at root ⇒ Nil.
                                Some(ViewMode::Nil)
                            } else {
                                // :5296.
                                focus.pop();
                                None
                            }
                        }
                        ViewMode::Map => {
                            // :5302-5305.
                            return Err(method_not_defined(kind, TYPE_PATHMAP));
                        }
                        ViewMode::Nil => unreachable!("Nil views return at step (c)"),
                    };
                    if let Some(next) = transition {
                        mode = next;
                    }
                }

                // ── toNextSibling (:5684-5740) / toPrevSibling (:5774-5830)
                LinkKind::ToNextSibling | LinkKind::ToPrevSibling => {
                    let transition = match &mut mode {
                        ViewMode::Zipper { focus, .. } => {
                            if focus.is_empty() {
                                // :5688-5690/:5778-5780 — no siblings at root.
                                Some(ViewMode::Nil)
                            } else {
                                let current_segment = focus
                                    .last()
                                    .expect("non-empty focus has a last segment")
                                    .clone();
                                let parent_key = flatten_key(&focus[..focus.len() - 1]);
                                // :5713/:5803 — all siblings, ascending
                                // byte-lex, deduplicated.
                                let siblings = collect_child_segments(
                                    &chain.interned.map,
                                    &parent_key,
                                    None,
                                );
                                match siblings.iter().position(|s| s == &current_segment) {
                                    Some(current_idx) => {
                                        let target_idx = if kind == LinkKind::ToNextSibling {
                                            // :5719-5729.
                                            (current_idx + 1 < siblings.len())
                                                .then_some(current_idx + 1)
                                        } else {
                                            // :5809-5819.
                                            current_idx.checked_sub(1)
                                        };
                                        match target_idx {
                                            Some(sibling_idx) => {
                                                focus.pop();
                                                focus.push(siblings[sibling_idx].clone());
                                                None
                                            }
                                            // No next/previous sibling ⇒ Nil.
                                            None => Some(ViewMode::Nil),
                                        }
                                    }
                                    // :5730-5733/:5820-5823 — current not
                                    // found ("shouldn't happen") ⇒ Nil.
                                    None => Some(ViewMode::Nil),
                                }
                            }
                        }
                        ViewMode::Map => {
                            // :5735-5738/:5825-5828.
                            return Err(method_not_defined(kind, TYPE_PATHMAP));
                        }
                        ViewMode::Nil => unreachable!("Nil views return at step (c)"),
                    };
                    if let Some(next) = transition {
                        mode = next;
                    }
                }

                // ── reset (:5236-5251) ──────────────────────────────────
                LinkKind::Reset => match &mut mode {
                    ViewMode::Zipper { focus, .. } => {
                        // :5240 — clear to root.
                        focus.clear();
                    }
                    ViewMode::Map => {
                        // :5246-5249.
                        return Err(method_not_defined(kind, TYPE_PATHMAP));
                    }
                    ViewMode::Nil => unreachable!("Nil views return at step (c)"),
                },

                // ── pathExists (:4990-5058) — TERMINAL ──────────────────
                LinkKind::PathExists => {
                    let exists = match &mode {
                        ViewMode::Zipper { focus, .. } => {
                            let key = flatten_key(focus);
                            if key.is_empty() {
                                // :5011-5013 — root exists iff the EMBEDDED
                                // message is non-empty; the embedded message
                                // is the source message (carried through
                                // navigation unchanged).
                                !chain.source_map.ps.is_empty()
                            } else {
                                // :5019 — native trie-path lookup.
                                path_prefix_exists(&chain.interned.map, &key)
                            }
                        }
                        // :5022-5024 — a raw map exists iff non-empty.
                        ViewMode::Map => !chain.source_map.ps.is_empty(),
                        ViewMode::Nil => unreachable!("Nil views return at step (c)"),
                    };
                    // :5055-5057.
                    return Ok(single_expr_par(ExprInstance::GBool(exists)));
                }

                // ── getLeaf (:3904-3977) — TERMINAL ─────────────────────
                LinkKind::GetLeaf => {
                    let value = match &mode {
                        ViewMode::Zipper { focus, .. } => {
                            // :3915-3930 — key from focus; absent ⇒ Nil.
                            let key = flatten_key(focus);
                            match chain.interned.map.get(&key) {
                                Some(value) => value.clone(),
                                None => Par::default(),
                            }
                        }
                        ViewMode::Map => {
                            // :3938-3950 — the raw-map ROOT variant, through
                            // the same RholangReadZipper path (root value or
                            // Nil).
                            let read_zipper = RholangReadZipper::new(
                                &chain.interned.map,
                                chain.interned.connective_used,
                                chain.interned.locally_free.clone(),
                            );
                            match read_zipper.get_val() {
                                Some(value) => value.clone(),
                                None => Par::default(),
                            }
                        }
                        ViewMode::Nil => unreachable!("Nil views return at step (c)"),
                    };
                    // :3976 — apply returns the leaf Par UNWRAPPED.
                    return Ok(value);
                }

                // ── getSubtrie (:3989-4056) — TERMINAL ──────────────────
                LinkKind::GetSubtrie => {
                    let result = match &mode {
                        ViewMode::Zipper { focus, .. } => {
                            // :3999-4013 — native subtrie descent below the
                            // focus prefix.
                            let elements =
                                collect_subtrie_values(&chain.interned.map, &flatten_key(focus));
                            // :4016-4023 — locally_free/connective_used from
                            // the CONVERSION result (the interned entry),
                            // remainder None.
                            single_expr_par(ExprInstance::EPathmapBody(EPathMap {
                                ps: elements,
                                locally_free: chain.interned.locally_free.clone(),
                                connective_used: chain.interned.connective_used,
                                remainder: None,
                            }))
                        }
                        ViewMode::Map => {
                            // :4025-4029 — the whole map back; today's arm
                            // returns the re-evaluated message, which is
                            // byte-identical to the source under the gate.
                            single_expr_par(ExprInstance::EPathmapBody(chain.source_map.clone()))
                        }
                        ViewMode::Nil => unreachable!("Nil views return at step (c)"),
                    };
                    return Ok(result);
                }

                // ── childCount (:5419-5490) — TERMINAL ──────────────────
                LinkKind::ChildCount => {
                    let count = match &mode {
                        ViewMode::Zipper { focus, .. } => {
                            // :5428-5444 — distinct immediate children below
                            // the focus.
                            collect_child_segments(&chain.interned.map, &flatten_key(focus), None)
                                .len() as i64
                        }
                        ViewMode::Map => {
                            // :5446-5457 — distinct first segments.
                            collect_child_segments(&chain.interned.map, &[], None).len() as i64
                        }
                        ViewMode::Nil => unreachable!("Nil views return at step (c)"),
                    };
                    // :5487-5489.
                    return Ok(single_expr_par(ExprInstance::GInt(count)));
                }

                // ── atPath (:4895-4977) — TERMINAL ──────────────────────
                LinkKind::AtPath => {
                    let path_par =
                        arg_b.as_ref().expect("arity-1 link must have a Position-B argument");
                    let key = match &mode {
                        ViewMode::Zipper { focus, .. } => {
                            // :4906-4919 — focus ++ argument path.
                            let mut full_path = focus.clone();
                            full_path.extend(par_to_path(path_par));
                            flatten_key(&full_path)
                        }
                        // :4935-4943 — argument path from root.
                        ViewMode::Map => flatten_key(&par_to_path(path_par)),
                        ViewMode::Nil => unreachable!("Nil views return at step (c)"),
                    };
                    // :4922-4925/:4945-4948 — value or Nil, UNWRAPPED.
                    return Ok(match chain.interned.map.get(&key) {
                        Some(value) => value.clone(),
                        None => Par::default(),
                    });
                }
            }
        }

        // (5) the chain ended on a view-preserving link: materialize.
        match mode {
            // A terminal Nil (e.g. `readZipper().ascendOne()`): today's apply
            // returned `Par::default()` from the failing navigation.
            ViewMode::Nil => Ok(Par::default()),
            // The exact EZipper Par today's per-link path produces — ONE map
            // embed cloned from the borrowed base message (parity, no win
            // claimed on this arm).
            ViewMode::Zipper { focus, meta } => {
                Ok(single_expr_par(ExprInstance::EZipperBody(EZipper {
                    pathmap: Some(chain.source_map.clone()),
                    current_path: focus,
                    is_write_zipper: meta.is_write,
                    locally_free: meta.locally_free,
                    connective_used: meta.connective_used,
                })))
            }
            ViewMode::Map => unreachable!(
                "a fused chain has at least one link, and no link leaves a map-mode view \
                 in map mode (readZipper/readZipperAt produce a zipper, value producers \
                 return, every other link raises MethodNotDefined)"
            ),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Test-only support (amendment PM-5(3)): compile-time-gated force-disable +
// shape-keyed fusion-hit counters
// ─────────────────────────────────────────────────────────────────────────────

/// TEST-ONLY seams. Compiled ONLY under `cfg(test)` or the
/// `epathmap-fusion-differential` feature (the differential integration
/// suite's compile-time gate) — production builds contain NO
/// runtime-flippable fusion path and NO counters.
#[cfg(any(test, feature = "epathmap-fusion-differential"))]
pub mod fusion_test_support {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Mutex, OnceLock};

    static FORCE_DISABLED: AtomicBool = AtomicBool::new(false);
    static TOTAL_HITS: AtomicU64 = AtomicU64::new(0);
    static HITS_BY_SHAPE: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();

    fn shape_map() -> &'static Mutex<HashMap<String, u64>> {
        HITS_BY_SHAPE.get_or_init(|| Mutex::new(HashMap::new()))
    }

    /// Force the recognizer to decline every chain (the differential
    /// harness's "unfused" mode). Test-build-only by construction.
    pub fn set_force_disabled(disabled: bool) {
        FORCE_DISABLED.store(disabled, Ordering::SeqCst);
    }

    pub fn force_disabled() -> bool {
        FORCE_DISABLED.load(Ordering::SeqCst)
    }

    /// Total fused-chain evaluations since process start (or the last
    /// [`reset_counters`]).
    pub fn total_fusion_hits() -> u64 {
        TOTAL_HITS.load(Ordering::SeqCst)
    }

    /// Per-shape hit counts, keyed `{base}:{innermost.….outermost}` (e.g.
    /// `var-map:readZipperAt.getSubtrie`).
    pub fn fusion_hits_by_shape() -> HashMap<String, u64> {
        shape_map()
            .lock()
            .expect("fusion-hit shape map mutex poisoned")
            .clone()
    }

    pub fn reset_counters() {
        TOTAL_HITS.store(0, Ordering::SeqCst);
        shape_map()
            .lock()
            .expect("fusion-hit shape map mutex poisoned")
            .clear();
    }

    pub(super) fn record_hit(shape_key: String) {
        TOTAL_HITS.fetch_add(1, Ordering::SeqCst);
        *shape_map()
            .lock()
            .expect("fusion-hit shape map mutex poisoned")
            .entry(shape_key)
            .or_insert(0) += 1;
    }
}
