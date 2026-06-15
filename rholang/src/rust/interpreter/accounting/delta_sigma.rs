//! Per-signature demand analyzer `Δ_s` and supply closure `Σ_s` — the PURE,
//! linear-time static analysis that the block-assembly acceptance gate (D2) runs
//! before any speculative execution (cost-accounted-rho paper, Def 17 `Δ_s`,
//! Def 18 `Σ_s`, Def 19 funding obligation, Thm 20 decidability + over-approx,
//! Remark 21 "≤1 of competing proofs succeeds"; §7.4 desugar-then-count).
//!
//! ## The s₀ collapse (load-bearing representation decision)
//!
//! The f1r3node runtime is the spec's `s₀` collapse (Remark 11): ONE envelope
//! `Sig` per deploy, installed once before evaluation, with the normalized `Par`
//! carrying NO per-layer signature annotation. So a static `Δ_s` has nothing to
//! count layers on — instead it counts the **token-consuming COMM reductions** in
//! the fully-desugared `Par`, attributing ALL of them to the deploy's envelope
//! signature (Def 7.4: each `{·}_σ` layer is attributed to the whole-signature
//! value σ — no per-component split; the Split/Join closure `effective_supply`
//! handles split-vs-combined granularity). The signature dimension comes from the
//! envelope `Sig` (from `Cosigned`, possibly a `Sig::And` compound); the layer
//! count comes from the desugared `Par`. For any signature `s ≠ envelope`,
//! `Δ_s = 0` (the collapsed deploy carries no layers attributed to a foreign
//! signature). See
//! `docs/theory/cost-accounting-impl/workstream-d-acceptance.md` ("Central
//! representation decision").
//!
//! ## The load-bearing equivalence (consensus-critical, gate↔runtime bridge)
//!
//! The static `Δ_s` MUST equal the runtime's actual consumed token count for a
//! funded deploy that runs to completion — the spec's "consumed = Δ_s", which
//! `replay_cost_mismatch` (replay_runtime.rs) guards as `total_cost == consumed`.
//! D3 (DR-9 one-token-per-COMM, OD-3): the runtime emits a
//! `BillableTokenEvent{kind: Comm}` at each token-consuming COMM (`eval_send`,
//! `eval_receive`) and a DIAGNOSTIC `BillableTokenEvent{kind: Reduction}` at
//! each non-COMM structural reduction (`eval_new`, `eval_match`, `eval_if`). The
//! consensus consumed cost (`reconcile_lane`) counts ONLY the `Comm` events —
//! one token per COMM — so over a fully-reducing deploy it equals the number of
//! `Send` + `Receive` nodes reachable in its `Par` (NOT `New`/`Match`/`If`).
//! [`demand`] counts that exact COMM node set. This equivalence is validated
//! against the live runtime in `rholang/tests/accounting/delta_sigma_spec.rs`
//! for the §7.4 debit/credit example (8 token-consuming COMMs) and the
//! Appendix-B 3-layer validator handler.
//!
//! ## `?!` / uniform-signing desugaring (§7.4 — "8 not 6")
//!
//! The §7.4 semantic count requires the synchronous-send sugar `x?!(args)` to be
//! expanded to `new ret in { x!(ret, args) | for(_ <- ret){ cont } }` — a send +
//! a for-comprehension on EACH side — so the count reflects the desugared form
//! the runtime executes (8), not the syntactic signed-layer count (6). The
//! f1r3node normalizer ALREADY performs this expansion: `?!` is desugared by
//! `compiler/normalizer/processes/p_send_sync_normalizer.rs` at normalization
//! time, so a normalized `Par` passed to [`demand`] already contains the
//! desugared send + for nodes. [`desugar_for_funding`] therefore does NOT
//! re-expand `?!` (that would double-count); it is the identity on an
//! already-normalized `Par` and exists to make the desugar contract explicit at
//! the funding boundary (see its doc comment). Uniform signing likewise needs no
//! expansion here: under the s₀ collapse the normalized `Par` carries no `{·}_s`
//! layers to nest, and every COMM is attributed to the envelope signature.
//!
//! ## Purity
//!
//! This module is PURE and linear-time: it operates on `Par` + `Sig` + integer
//! supply maps only — no RSpace, no async, no I/O. The raw per-signature supply
//! values `Σ_s` are read elsewhere (the D2 gate, via
//! `casper/.../util/rholang/supply.rs::read_balance`) and fed in as a
//! `BTreeMap<SigKey, i64>`; this module never decodes a balance datum itself
//! (supply-realization handoff Decision 5 — one shared decoder).

use std::collections::BTreeMap;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::Par;

use super::Sig;

/// Canonical per-signature map key. Equal to `Sig::lane_hash` — the SAME
/// canonical, axis-independent, permutation-invariant digest WD-D0's lane pool
/// (`accounting/mod.rs`) keys lanes by and that StageB's `supply_channel`
/// (`SignatureChannel::from_sig`) anchors the supply channel `Σ⟦s⟧` to
/// (integration invariant — one canonical basis, no drift). Keying the
/// `effective_supply` map by this digest means the gate's per-group supply
/// lookups, the runtime's lane keys, and the on-chain supply channel all agree.
pub type SigKey = [u8; 32];

/// Compute the canonical `SigKey` for a signature (its `Sig::lane_hash`). Thin
/// re-export so callers (the D2 gate, the supply consumer) key the supply map by
/// the same basis without reaching into `accounting/mod.rs` internals.
#[inline]
pub fn sig_key(sig: &Sig) -> SigKey { sig.lane_hash() }

/// The static demand analysis result for one signature `s` over a desugared
/// `Par` (cost-accounted-rho Def 17 + Thm 20 over-approximation).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DemandEntry {
    /// `known_lower_bound = Δ_s` over the statically-resolvable part of the term:
    /// the number of token-consuming COMM reductions (the runtime's `Comm`
    /// nodes — send / receive ONLY; D3/OD-3 excludes new / match / if, which are
    /// DIAGNOSTIC `Reduction`s) attributed to `s`. Under the s₀ collapse this is
    /// the whole desugared `Par`'s COMM-node count when `s` is the envelope
    /// signature, and `0` for any other signature. `i64` to match the supply
    /// unit (`Σ_s`, a balance) so the funding comparison is one integer
    /// inequality in identical units.
    pub known_lower_bound: i64,
    /// `true` iff the term contains an unresolvable dequotation `*x` (a `Drop` /
    /// eval of a name whose bound process is not statically known — a `bound_var`
    /// / `free_var` left un-inlined by the normalizer, or a higher-order channel
    /// pass). Per Thm 20 the analysis then degrades to a conservative
    /// over-approximation: the true demand is AT LEAST `known_lower_bound` but may
    /// be larger, so the gate must reject unless the supply clears the lower
    /// bound plus a safety margin (see [`is_funded`]).
    pub unknown: bool,
}

impl DemandEntry {
    /// The zero demand (no token-consuming COMMs, fully resolvable). Identity for
    /// [`DemandEntry::combine`].
    pub const ZERO: DemandEntry = DemandEntry {
        known_lower_bound: 0,
        unknown: false,
    };

    /// Parallel/sequential composition of two sub-results: demands add
    /// (Def 17 `Δ_s(T | U) = Δ_s(T) + Δ_s(U)`), and `unknown` is sticky (any
    /// unresolvable sub-term makes the whole term's demand an over-approximation).
    /// Saturating add keeps the bound well-defined even for adversarially huge
    /// ASTs (the AST size is bounded upstream by the term-count limit in
    /// `reduce.rs::eval_inner`, but saturation is the safe direction regardless).
    #[inline]
    fn combine(self, other: DemandEntry) -> DemandEntry {
        DemandEntry {
            known_lower_bound: self
                .known_lower_bound
                .saturating_add(other.known_lower_bound),
            unknown: self.unknown || other.unknown,
        }
    }

    /// Add one token-consuming COMM reduction to the known lower bound.
    #[inline]
    fn plus_one(self) -> DemandEntry {
        DemandEntry {
            known_lower_bound: self.known_lower_bound.saturating_add(1),
            unknown: self.unknown,
        }
    }
}

/// `Δ_s(desugared)` — the per-signature token demand of a fully-desugared `Par`
/// with respect to the deploy's envelope signature (Def 17 under the s₀
/// collapse). Counts every token-consuming COMM reduction reachable in `par`,
/// attributing ALL of them to `deploy_sig` (Def 7.4 whole-signature
/// attribution). Returns the known lower bound plus an `unknown` flag set when an
/// unresolvable `*x` is encountered (Thm 20).
///
/// Linear time in the size of the AST: a single structural pass, O(1) work per
/// node, no normalization or fixpoint.
///
/// The set of counted nodes is exactly the set on which the runtime emits a
/// `BillableTokenEvent{kind: Comm}` (see module docs): `Send`, `Receive`. D3
/// (DR-9, OD-3): `New`, `Match`, `If` are DIAGNOSTIC `Reduction`s — they are
/// RECURSED (their process-position bodies fire COMMs) but do NOT themselves
/// contribute a counted node. An `EMethodBody` expression is NOT a COMM (the
/// runtime charges it as a `Primitive`) but its receiver/arguments may contain
/// nested processes, so it is recursed without contributing a node. An
/// `EVarBody` in process position that is a `bound_var` / `free_var` is an
/// un-inlined `*name` dequotation — the over-approximation trigger.
///
/// Note on the s₀ collapse and the `deploy_sig` parameter: because the
/// normalized `Par` carries no per-layer signature, EVERY COMM is attributed to
/// the single envelope signature. For any `s ≠ deploy_sig`, `Δ_s` over the same
/// `Par` is `0` by definition — there are no `s`-attributed layers in a deploy
/// signed by `deploy_sig`. Callers compute `Δ_s` for the deploy's own envelope
/// signature; the `deploy_sig` argument is threaded for documentation/typing
/// clarity and to make the attribution explicit at the call site (it does not
/// change the count, which is signature-agnostic under the collapse).
pub fn demand(desugared: &Par, deploy_sig: &Sig) -> DemandEntry {
    // `deploy_sig` participates in the s₀-collapse attribution semantics
    // (every counted COMM belongs to this signature's lane); the count itself
    // is structural and does not branch on the signature's shape.
    let _ = deploy_sig;
    demand_par(desugared)
}

/// Structural `Δ_s` over a `Par` in PROCESS position — the bag of parallel
/// sub-processes that are actually reduced (Def 17 `Δ_s(T | U) = Δ_s(T) +
/// Δ_s(U)`). Counts one token-consuming reduction per COMM-driving node and
/// recurses ONLY into the node's process-position continuation(s).
///
/// What is and is NOT a process position (this is the rule that makes the count
/// equal the runtime's `SourceStep` count, validated empirically against the
/// reducer in `delta_sigma_spec.rs`):
///   * RECURSED (process positions, executed by the reducer): each Par member of
///     the top-level parallel bag; a receive's continuation `body`; a `new`'s
///     scoped body; a match's/if's case continuations and branches; a bundle's
///     body.
///   * NOT recursed (name/key positions, NEVER reduced as a process): a send's
///     CHANNEL and DATA payloads; a receive bind's SOURCE channel and PATTERNS;
///     a match's SCRUTINEE target. These are tuplespace keys / message values /
///     match subjects — the reducer treats them as data, not as running
///     processes, so their internal sends/receives do NOT fire and contribute
///     ZERO token-consuming COMMs. (A quoted process placed in such a position
///     only ever runs if it is later dequoted via `*x` in a PROCESS position,
///     which is then accounted as an unresolved drop ⇒ `unknown` below.)
///
/// This name-vs-process discipline is also why a bound name in channel position
/// (the normal way to reference a `new`-bound channel, an `EVar(bound_var)` in a
/// channel `Par`) must NOT trigger the `unknown` over-approximation: it is a name
/// reference, not a process dequotation. Only an `EVar(bound_var|free_var)`
/// appearing as a top-level PROCESS member is a `*x` drop.
fn demand_par(par: &Par) -> DemandEntry {
    let mut acc = DemandEntry::ZERO;

    // Sends: `Δ_s(send x U) = Δ_s(U)` — but the runtime sends `U` as a message
    // value (it does not reduce it), so the only token-consuming reduction is the
    // send itself (`eval_send` → one SourceStep). The channel and data are name /
    // value positions: NOT recursed (they contribute zero COMMs).
    for _send in &par.sends {
        acc = acc.plus_one();
    }

    // Receives: `Δ_s(for y x T) = Δ_s(T)`, plus one token for the receive
    // reduction (`eval_receive` → one SourceStep). The bind sources and patterns
    // are name positions (NOT recursed); the continuation `body` IS a process
    // position (it fires once the COMM commits — the multi-step transaction the
    // funding proof exists to fully fund), so it is recursed.
    for receive in &par.receives {
        let mut node = DemandEntry::ZERO.plus_one();
        if let Some(body) = &receive.body {
            node = node.combine(demand_par(body));
        }
        acc = acc.combine(node);
    }

    // New: name allocation. D3 (DR-9, OD-3): the runtime meters `eval_new` as a
    // DIAGNOSTIC `Reduction`, NOT a `Comm` — so it contributes ZERO to the
    // per-COMM consensus demand. We still RECURSE into the scoped body (a
    // process position whose COMMs fire), but the `new` node itself no longer
    // counts. This is the §7.4 "9 → 8" re-pin: the `new` no longer adds a token.
    for new in &par.news {
        if let Some(body) = &new.p {
            acc = acc.combine(demand_par(body));
        }
    }

    // Match: D3 (DR-9, OD-3): the runtime meters `eval_match` as a DIAGNOSTIC
    // `Reduction`, NOT a `Comm` — ZERO toward the per-COMM consensus demand.
    // The scrutinee `target` is a value position (NOT recursed); each case's
    // continuation `source` IS a process position (the matched branch fires its
    // COMMs): recurse without counting the match node.
    for mat in &par.matches {
        for case in &mat.cases {
            if let Some(source) = &case.source {
                acc = acc.combine(demand_par(source));
            }
        }
    }

    // If: first-class conditional. D3 (DR-9, OD-3): the runtime meters `eval_if`
    // as a DIAGNOSTIC `Reduction`, NOT a `Comm` — ZERO toward the per-COMM
    // consensus demand. The `condition` is a value position (NOT recursed); both
    // branches ARE process positions: recurse without counting the if node.
    for conditional in &par.conditionals {
        if let Some(if_true) = &conditional.if_true {
            acc = acc.combine(demand_par(if_true));
        }
        if let Some(if_false) = &conditional.if_false {
            acc = acc.combine(demand_par(if_false));
        }
    }

    // Bundles wrap a body in a read/write capability annotation; the bundle
    // itself is not a COMM (no SourceStep), but its body IS a process position
    // whose COMMs fire once unbundled. Recurse without contributing a node.
    for bundle in &par.bundles {
        if let Some(body) = &bundle.body {
            acc = acc.combine(demand_par(body));
        }
    }

    // Expressions in PROCESS position. Most are pure values (no COMM). The one
    // process-relevant case is an `EVarBody` that is an un-inlined `*name`
    // dequotation (`Δ_s(*x) = Δ_s^resolve(x)`): the normalizer inlines `*@P`
    // (a quoted process) directly, so any surviving `EVar(bound_var|free_var)`
    // here is a name whose bound process is not statically known — the Thm 20
    // over-approximation trigger (`unknown = true`). A wildcard is inert.
    // (`EMethodBody` is charged as a `Primitive`, not a `SourceStep`, and its
    // receiver/arguments are value positions, so it contributes nothing here.)
    for expr in &par.exprs {
        if let Some(ExprInstance::EVarBody(evar)) = &expr.expr_instance {
            if let Some(var) = &evar.v {
                match &var.var_instance {
                    Some(VarInstance::BoundVar(_)) | Some(VarInstance::FreeVar(_)) => {
                        acc = acc.combine(DemandEntry {
                            known_lower_bound: 0,
                            unknown: true,
                        });
                    }
                    _ => {}
                }
            }
        }
    }

    // `connectives` and `unforgeables` carry no token-consuming COMMs:
    // connectives are logical pattern combinators (meaningful only inside
    // patterns, which are name positions), and a `GPrivate` unforgeable is an
    // opaque name with no sub-process. Both are intentionally not recursed.

    acc
}

/// §7.4 desugaring boundary for the funding analysis. The §7.4 semantic count
/// ("8 not 6") requires `?!` (synchronous send) to be expanded to a send + a
/// for-comprehension on each side, and uniform signing to be expanded to its
/// nested signed layers — so that [`demand`] counts the form the runtime
/// actually executes rather than the syntactic surface.
///
/// In f1r3node this expansion is performed UPSTREAM by the normalizer: `?!` is
/// desugared by `compiler/normalizer/processes/p_send_sync_normalizer.rs` into
/// `new ret in { chan!(ret, args) | for(_ <- ret){ cont } }`, and the s₀ collapse
/// means the normalized `Par` carries no `{·}_s` layers for uniform signing to
/// nest. A `Par` produced by `Compiler::source_to_adt` (the same path the
/// runtime evaluates through) is therefore ALREADY in the desugared form
/// [`demand`] requires. Re-expanding here would double-count the send/for nodes.
///
/// This function is consequently the identity on an already-normalized `Par`. It
/// exists to (a) make the desugar-then-count contract explicit at the funding
/// boundary, and (b) provide a single, named seam to extend should a future
/// front-end deliver a `Par` that is NOT pre-desugared (none does today). The
/// returned value is `Cow`-free (an owned clone) to keep the boundary a total
/// function over `&Par`.
#[inline]
pub fn desugar_for_funding(par: &Par) -> Par { par.clone() }

/// The Split/Join supply closure `effectiveΣ` (cost-accounted-rho §B.1
/// decomposition equivalence; Appendix A eq:app-st-signed-compound). Given the
/// RAW per-signature supplies `Σ_s` (each a balance `n`, keyed by `Sig::lane_hash`
/// — read by the gate via `supply::read_balance`), produce the EFFECTIVE supplies
/// that account for the interchangeability between a combined compound stack
/// `s₁∘s₂` and the minimum of its component stacks:
///
/// ```text
/// effectiveΣ_{s₁∘s₂} = Σ_{s₁∘s₂} + min(Σ_{s₁}, Σ_{s₂})
/// effectiveΣ_{s₁}    = Σ_{s₁}    + Σ_{s₁∘s₂}
/// ```
///
/// Intuition: a deploy demanding the compound `s₁∘s₂` may draw either from the
/// compound pool directly OR from a matched pair of component tokens — so its
/// effective compound supply is the compound balance plus the number of pairs the
/// components can form (`min`). Dually, a deploy demanding a single component
/// `s₁` may draw from `s₁`'s own pool OR from the compound pool (a compound token
/// satisfies a component obligation) — so its effective single supply is the sum.
///
/// ## Realization under the s₀ collapse (grounding adaptation — reported)
///
/// The spec states this closure over the abstract signature algebra `s₁∘s₂`. At
/// the substrate, the only compound the runtime forms is `Sig::And` (the proto
/// `Tensor`); a deploy's envelope is either a single atom or a `Sig::And` of two
/// (or, via `Threshold`, more) atoms. This function reconstructs the closure from
/// the raw-supply map by, for each compound key present, locating its component
/// keys (themselves derivable as `Sig::lane_hash` of the components) and applying
/// the two equations. Because the input map is keyed by opaque `SigKey` digests
/// (not structured `Sig`s — the gate reads balances by channel digest), the
/// closure is computed structurally from a companion list of the in-scope
/// signatures supplied by the caller. To keep this function a PURE map→map
/// transform with no `Sig`-reconstruction-from-digest (which is not invertible),
/// the closure is expressed over an explicit `decomposition` describing which
/// compound key splits into which two component keys; the no-decomposition case
/// (a flat map of independent atoms) is the identity, which is the common
/// single-signature and disjoint-multi-signature fast path.
///
/// The caller (D2 gate) — which HAS the structured envelope `Sig`s in hand —
/// builds the `decomposition` by walking each `Sig::And`/compound envelope and
/// emitting `(lane_hash(compound), lane_hash(left), lane_hash(right))`. Atoms and
/// already-disjoint signatures contribute no decomposition entry and pass through
/// unchanged.
pub fn effective_supply(raw: &BTreeMap<SigKey, i64>) -> BTreeMap<SigKey, i64> {
    // With no decomposition information, every signature is treated as an
    // independent atom: `effectiveΣ_s = Σ_s` (the closure's identity case). This
    // is the single-signature fast path and the disjoint-multi-signature path,
    // where no compound pool exists to fold in.
    effective_supply_with(raw, &[])
}

/// Describes one Split/Join decomposition: a compound signature's supply key and
/// the two component keys it splits into (cost-accounted-rho §B.1). Built by the
/// gate from a structured compound envelope (`lane_hash(compound)`,
/// `lane_hash(left)`, `lane_hash(right)`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decomposition {
    /// `lane_hash(s₁∘s₂)` — the compound supply key.
    pub compound: SigKey,
    /// `lane_hash(s₁)` — the left component supply key.
    pub left: SigKey,
    /// `lane_hash(s₂)` — the right component supply key.
    pub right: SigKey,
}

/// The Split/Join closure with explicit compound decompositions (the general
/// form of [`effective_supply`]). Applies, for each decomposition
/// `(s₁∘s₂, s₁, s₂)`:
///
/// ```text
/// effectiveΣ_{s₁∘s₂} = Σ_{s₁∘s₂} + min(Σ_{s₁}, Σ_{s₂})
/// effectiveΣ_{s₁}    = Σ_{s₁}    + Σ_{s₁∘s₂}
/// effectiveΣ_{s₂}    = Σ_{s₂}    + Σ_{s₁∘s₂}
/// ```
///
/// using the RAW balances (`min` is computed on the raw component supplies so the
/// closure is well-defined regardless of decomposition order — the result is a
/// pure function of `raw` and the decomposition set). Keys not mentioned in any
/// decomposition pass through with `effectiveΣ_s = Σ_s`. A balance absent from
/// `raw` reads as `0` (supply-realization Decision 2 — 0 when absent).
pub fn effective_supply_with(
    raw: &BTreeMap<SigKey, i64>,
    decompositions: &[Decomposition],
) -> BTreeMap<SigKey, i64> {
    // Start from the identity (effectiveΣ_s = Σ_s for every present key), then
    // fold in each compound's contribution. Reading raw balances throughout
    // (never the partially-updated `effective`) keeps the closure order-
    // independent and a pure function of (raw, decompositions).
    let mut effective = raw.clone();

    let read_raw = |key: &SigKey| -> i64 { raw.get(key).copied().unwrap_or(0) };

    for decomposition in decompositions {
        let sigma_compound = read_raw(&decomposition.compound);
        let sigma_left = read_raw(&decomposition.left);
        let sigma_right = read_raw(&decomposition.right);

        // effectiveΣ_{s₁∘s₂} = Σ_{s₁∘s₂} + min(Σ_{s₁}, Σ_{s₂})
        let compound_effective = sigma_compound.saturating_add(sigma_left.min(sigma_right));
        effective.insert(decomposition.compound, compound_effective);

        // effectiveΣ_{s₁} = Σ_{s₁} + Σ_{s₁∘s₂}  (and dually for s₂)
        effective.insert(
            decomposition.left,
            sigma_left.saturating_add(sigma_compound),
        );
        effective.insert(
            decomposition.right,
            sigma_right.saturating_add(sigma_compound),
        );
    }

    effective
}

/// The funding decision for one signature group (cost-accounted-rho Def 19 +
/// Thm 20): a deploy (or canonical-order prefix of a signature group) is fundable
/// iff the EFFECTIVE supply meets or exceeds the demand — plus, for
/// over-approximated (`unknown`) demand only, a safety margin.
///
/// ```text
/// fundable  ⇔  effective_supply_s ≥ known_lower_bound + (margin if unknown else 0)
/// ```
///
/// Two regimes:
///   * Fully resolvable demand (`unknown == false`): `known_lower_bound` IS the
///     exact `Δ_s`, so the check is EXACTLY Def 19 `Σ_s ≥ Δ_s` — NO margin. The
///     economic floor `min_phlo_price` is deliberately NOT folded into the
///     resolvable-demand correctness gate (an economic surcharge is not a
///     correctness condition; matches the Rocq model `funds n d := d ≤ n`).
///   * Over-approximated demand (`unknown == true`): the true demand exceeds
///     `known_lower_bound` by an unknown amount, so the deploy is admitted ONLY
///     when the supply clears the lower bound plus the margin — the conservative
///     SAFE direction (Thm 20: "the validator rejects unless the supply exceeds
///     the known lower bound plus a configurable safety margin"). When the
///     inequality fails, an un-analyzable deploy is rejected.
///
/// `margin` is a SHARD-GENESIS constant (parameter here; the D2 gate supplies the
/// genesis value — it is NOT hardcoded in this pure module). A non-positive
/// `margin` reduces the check to the bare `Σ_s ≥ Δ_s`.
///
/// The comparison is done in `i128` to defend against an adversarial
/// `known_lower_bound + margin` overflow (both are bounded in practice, but the
/// gate must never wrap into acceptance).
#[inline]
pub fn is_funded(analysis: &DemandEntry, effective_supply_s: i64, margin: i64) -> bool {
    // Def 19 (`Σ_s ≥ Δ_s`) for resolvable demand; the Thm 20 safety margin applies
    // ONLY to the data-dependent over-approximation (`unknown == true`). The
    // economic floor `min_phlo_price` is NOT folded into the resolvable-demand
    // correctness gate (matches the verified Rocq model `funds n d := d ≤ n`,
    // which has no margin term).
    let applied_margin = if analysis.unknown { margin } else { 0 };
    let required = i128::from(analysis.known_lower_bound) + i128::from(applied_margin);
    i128::from(effective_supply_s) >= required
}

#[cfg(test)]
mod tests {
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::var::VarInstance;
    use models::rhoapi::{EVar, Expr, New, Par, Receive, ReceiveBind, Send, Var};

    use super::*;

    fn atom(tag: u8) -> Sig { Sig::Ground(vec![tag, tag, tag, tag]) }

    fn empty_par() -> Par { Par::default() }

    fn send_on(chan: Par) -> Send {
        Send {
            chan: Some(chan),
            data: Vec::new(),
            persistent: false,
            locally_free: Vec::new(),
            connective_used: false,
        }
    }

    fn par_with_sends(n: usize) -> Par {
        let mut par = Par::default();
        for _ in 0..n {
            par.sends.push(send_on(empty_par()));
        }
        par
    }

    // ── demand: structural counting ────────────────────────────────────────

    #[test]
    fn empty_par_has_zero_demand() {
        let entry = demand(&empty_par(), &atom(1));
        assert_eq!(entry.known_lower_bound, 0);
        assert!(!entry.unknown);
    }

    #[test]
    fn each_send_counts_one() {
        let entry = demand(&par_with_sends(3), &atom(1));
        assert_eq!(entry.known_lower_bound, 3);
        assert!(!entry.unknown);
    }

    #[test]
    fn receive_counts_self_plus_body() {
        // for(_ <- chan){ chan2!() | chan3!() }  ⇒ 1 (receive) + 2 (body sends).
        let mut body = Par::default();
        body.sends.push(send_on(empty_par()));
        body.sends.push(send_on(empty_par()));
        let receive = Receive {
            binds: vec![ReceiveBind {
                patterns: Vec::new(),
                source: Some(empty_par()),
                remainder: None,
                free_count: 0,
            }],
            body: Some(body),
            persistent: false,
            peek: false,
            bind_count: 0,
            locally_free: Vec::new(),
            connective_used: false,
            condition: None,
        };
        let mut par = Par::default();
        par.receives.push(receive);
        let entry = demand(&par, &atom(1));
        assert_eq!(entry.known_lower_bound, 3);
        assert!(!entry.unknown);
    }

    #[test]
    fn new_does_not_count_but_recurses_scoped_body() {
        // D3 (DR-9, OD-3): `new x in { send | send }` ⇒ 2 (the two body sends
        // only). The `new` node is a DIAGNOSTIC `Reduction`, NOT a `Comm`, so it
        // contributes 0; its scoped body is still recursed. This is the §7.4
        // "9 → 8" re-pin in miniature (the `new` no longer adds a token).
        let new = New {
            bind_count: 1,
            p: Some(par_with_sends(2)),
            uri: Vec::new(),
            injections: Default::default(),
            locally_free: Vec::new(),
        };
        let mut par = Par::default();
        par.news.push(new);
        let entry = demand(&par, &atom(1));
        assert_eq!(entry.known_lower_bound, 2);
        assert!(!entry.unknown);
    }

    #[test]
    fn data_payload_processes_are_not_counted() {
        // chan!( @{ send } )  ⇒ 1 (outer send only). The quoted process in the
        // data payload is a MESSAGE VALUE, not a running process — the reducer
        // sends it without reducing it, so it fires zero COMMs (it would only run
        // if later dequoted via `*x`, which is then accounted as `unknown`). This
        // matches the runtime's `SourceStep` count exactly (validated in
        // `delta_sigma_spec.rs`).
        let mut send = send_on(empty_par());
        send.data.push(par_with_sends(1));
        let mut par = Par::default();
        par.sends.push(send);
        let entry = demand(&par, &atom(1));
        assert_eq!(entry.known_lower_bound, 1);
        assert!(!entry.unknown);
    }

    #[test]
    fn bound_name_in_channel_position_is_not_unknown() {
        // for(y <- x){ Nil } where x is a `new`-bound name: the source channel is
        // an EVar(bound_var) in NAME position. That is a channel reference, NOT a
        // `*x` process dequotation, so it must NOT trigger `unknown`. The receive
        // counts 1 (itself); the empty body counts 0.
        let receive = Receive {
            binds: vec![ReceiveBind {
                patterns: Vec::new(),
                // x as a bound-var name in the source (channel) position.
                source: Some(eval_of_bound_var(0)),
                remainder: None,
                free_count: 0,
            }],
            body: Some(empty_par()),
            persistent: false,
            peek: false,
            bind_count: 0,
            locally_free: Vec::new(),
            connective_used: false,
            condition: None,
        };
        let mut par = Par::default();
        par.receives.push(receive);
        let entry = demand(&par, &atom(1));
        assert_eq!(entry.known_lower_bound, 1);
        assert!(
            !entry.unknown,
            "a bound name in channel position is not an unresolved dequotation"
        );
    }

    // ── demand: the unknown (over-approximation) trigger ───────────────────

    fn eval_of_bound_var(level: i32) -> Par {
        // `*x` where x is a bound name → an un-inlined EVar(bound_var) in
        // process position.
        let mut par = Par::default();
        par.exprs.push(Expr {
            expr_instance: Some(ExprInstance::EVarBody(EVar {
                v: Some(Var {
                    var_instance: Some(VarInstance::BoundVar(level)),
                }),
            })),
        });
        par
    }

    #[test]
    fn unresolved_eval_sets_unknown() {
        let entry = demand(&eval_of_bound_var(0), &atom(1));
        assert!(entry.unknown);
        // The dereference itself contributes no known COMM node — the demand is
        // entirely the (unknown) resolved process.
        assert_eq!(entry.known_lower_bound, 0);
    }

    #[test]
    fn unknown_is_sticky_across_parallel_composition() {
        // send | *x  ⇒ known 1 send, unknown true.
        let mut par = par_with_sends(1);
        par.exprs = eval_of_bound_var(0).exprs;
        let entry = demand(&par, &atom(1));
        assert_eq!(entry.known_lower_bound, 1);
        assert!(entry.unknown);
    }

    // ── desugar_for_funding: identity on a normalized Par ──────────────────

    #[test]
    fn desugar_for_funding_is_identity_on_normalized_par() {
        let par = par_with_sends(2);
        assert_eq!(desugar_for_funding(&par), par);
    }

    // ── effective_supply: the Split/Join closure arithmetic ────────────────

    #[test]
    fn effective_supply_identity_when_no_decomposition() {
        let mut raw = BTreeMap::new();
        raw.insert([1u8; 32], 5_i64);
        raw.insert([2u8; 32], 7_i64);
        let effective = effective_supply(&raw);
        assert_eq!(effective, raw);
    }

    #[test]
    fn effective_supply_split_join_closure_arithmetic() {
        // Σ_{s1} = 4, Σ_{s2} = 6, Σ_{s1∘s2} = 10.
        // effectiveΣ_{s1∘s2} = 10 + min(4,6) = 14
        // effectiveΣ_{s1}    = 4 + 10        = 14
        // effectiveΣ_{s2}    = 6 + 10        = 16
        let s1 = [1u8; 32];
        let s2 = [2u8; 32];
        let compound = [3u8; 32];
        let mut raw = BTreeMap::new();
        raw.insert(s1, 4_i64);
        raw.insert(s2, 6_i64);
        raw.insert(compound, 10_i64);

        let effective = effective_supply_with(&raw, &[Decomposition {
            compound,
            left: s1,
            right: s2,
        }]);

        assert_eq!(effective.get(&compound), Some(&14));
        assert_eq!(effective.get(&s1), Some(&14));
        assert_eq!(effective.get(&s2), Some(&16));
    }

    #[test]
    fn effective_supply_treats_absent_component_as_zero() {
        // Only the compound pool exists; components absent ⇒ read as 0.
        // effectiveΣ_{s1∘s2} = 8 + min(0,0) = 8
        // effectiveΣ_{s1}    = 0 + 8         = 8
        let s1 = [1u8; 32];
        let s2 = [2u8; 32];
        let compound = [3u8; 32];
        let mut raw = BTreeMap::new();
        raw.insert(compound, 8_i64);

        let effective = effective_supply_with(&raw, &[Decomposition {
            compound,
            left: s1,
            right: s2,
        }]);

        assert_eq!(effective.get(&compound), Some(&8));
        assert_eq!(effective.get(&s1), Some(&8));
        assert_eq!(effective.get(&s2), Some(&8));
    }

    #[test]
    fn effective_supply_closure_is_order_independent() {
        // The closure reads raw balances throughout, so applying two
        // decompositions in either order yields the same map.
        let a = [1u8; 32];
        let b = [2u8; 32];
        let ab = [3u8; 32];
        let c = [4u8; 32];
        let d = [5u8; 32];
        let cd = [6u8; 32];
        let mut raw = BTreeMap::new();
        for (k, v) in [(a, 2), (b, 3), (ab, 1), (c, 7), (d, 5), (cd, 4)] {
            raw.insert(k, v as i64);
        }
        let decomposition_ab = Decomposition {
            compound: ab,
            left: a,
            right: b,
        };
        let decomposition_cd = Decomposition {
            compound: cd,
            left: c,
            right: d,
        };

        let forward = effective_supply_with(&raw, &[decomposition_ab, decomposition_cd]);
        let backward = effective_supply_with(&raw, &[decomposition_cd, decomposition_ab]);
        assert_eq!(forward, backward);
    }

    // ── is_funded: Def 19 + Thm 20 over-approximation, ±margin boundary ────

    fn resolvable(lower: i64) -> DemandEntry {
        DemandEntry {
            known_lower_bound: lower,
            unknown: false,
        }
    }

    fn unresolvable(lower: i64) -> DemandEntry {
        DemandEntry {
            known_lower_bound: lower,
            unknown: true,
        }
    }

    #[test]
    fn resolvable_funded_at_def19_boundary_margin_inert() {
        // F-B: for resolvable demand the gate is EXACTLY `Σ ≥ Δ` — the economic
        // margin is NOT applied. Δ=8 ⇒ funded at Σ ≥ 8 for ANY margin.
        assert!(is_funded(&resolvable(8), 8, 0)); // Σ = Δ
        assert!(is_funded(&resolvable(8), 8, 2)); // Σ = Δ, margin inert
        assert!(is_funded(&resolvable(8), 9, 2)); // was REJECTED before the F-B fix
        assert!(is_funded(&resolvable(8), 100, 1000));
    }

    #[test]
    fn resolvable_rejected_below_demand_margin_does_not_shift_boundary() {
        // Σ < Δ ⇒ rejected; a non-zero margin must NOT raise the bar for resolvable
        // demand (before the F-B fix it would have required Σ ≥ Δ+margin).
        assert!(!is_funded(&resolvable(8), 7, 0)); // Σ = Δ-1
        assert!(!is_funded(&resolvable(8), 7, 2)); // margin inert ⇒ still just Σ < Δ
    }

    #[test]
    fn zero_margin_reduces_to_supply_ge_demand() {
        assert!(is_funded(&resolvable(3), 3, 0)); // Σ = Δ
        assert!(!is_funded(&resolvable(3), 2, 0)); // Σ < Δ
    }

    #[test]
    fn unknown_demand_applies_margin_over_known_lower_bound() {
        // The unknown flag is EXACTLY what gates the Thm 20 margin: an
        // over-approximated deploy is admitted ONLY when the supply clears the
        // KNOWN lower bound PLUS the margin (the safe direction), because the
        // lower bound under-states the true demand. (A resolvable deploy with the
        // same lower bound is funded at Σ ≥ Δ with NO margin — see the resolvable
        // tests above.) Δ_known=5, margin=4 ⇒ need Σ ≥ 9.
        assert!(is_funded(&unresolvable(5), 9, 4));
        assert!(!is_funded(&unresolvable(5), 8, 4));
    }

    #[test]
    fn unknown_reject_at_margin_boundary_pair() {
        // Exactly at the ±margin boundary for an unknown demand: Σ = Δ_known
        // (margin not cleared) ⇒ reject; Σ = Δ_known + margin ⇒ accept.
        let analysis = unresolvable(6);
        let margin = 3;
        assert!(!is_funded(&analysis, 6, margin)); // Σ = Δ_known, margin unmet
        assert!(!is_funded(&analysis, 8, margin)); // Σ = Δ_known + 2, still < +margin
        assert!(is_funded(&analysis, 9, margin)); // Σ = Δ_known + margin, accepted
    }

    #[test]
    fn is_funded_does_not_overflow_on_extreme_margin() {
        // An adversarial margin near i64::MAX must not wrap into acceptance. The
        // margin is added ONLY for UNKNOWN demand, so the overflow guard is
        // exercised with an unresolvable entry (`known_lower_bound + margin`
        // computed in i128 must never wrap an i64 into acceptance).
        assert!(!is_funded(&unresolvable(i64::MAX), i64::MAX, i64::MAX));
        // A genuinely-sufficient supply still funds a tiny demand.
        assert!(is_funded(&resolvable(1), i64::MAX, 0));
    }

    // ── sig_key: agrees with Sig::lane_hash ────────────────────────────────

    #[test]
    fn sig_key_equals_lane_hash() {
        let sig = atom(7);
        assert_eq!(sig_key(&sig), sig.lane_hash());
    }

    // ── #17 funding slots (§4.7): a slot signature flows through the SAME
    //    generic demand / supply / keying machinery as any ground signature ──

    #[test]
    fn funding_slot_signature_flows_through_generic_demand_and_funding() {
        // §4.7: a funding slot is a fresh unforgeable `new`-created name used AS
        // a signature (`{for(y<-x)P}_{s₁ ⊸ slot}`). Under the s₀ collapse the slot
        // is just another envelope `Sig`, so Δ_s counts the deploy's COMM nodes,
        // funding is Def 19 `Σ_slot ≥ Δ_slot` (resolvable ⇒ margin inert), an ABSENT slot pool (Σ = 0)
        // rejects a positive demand (§7.6 strict reject — "checks tokens on the
        // slot"), and the slot is keyed by the SAME canonical `lane_hash`/`from_sig`
        // basis as any ground signature. This pins the funding-slot path, which
        // was previously only inferred from the generic machinery (not tested).
        let slot = Sig::Ground(vec![0x5a; 32]); // a fresh slot name used as a signature
        let other_slot = Sig::Ground(vec![0x5b; 32]);
        let client = atom(1);
        // `s₁ ⊸ slot` resolves, at the gate, to the compound envelope `s₁ ∘ slot`.
        let compound = Sig::And(Box::new(client.clone()), Box::new(slot.clone()));

        // K token-consuming COMM nodes (sends) in the desugared body.
        let k: i64 = 3;
        let par = par_with_sends(k as usize);

        // Δ_slot counts the COMM nodes and is fully resolvable (no `*x`).
        let d_slot = demand(&par, &slot);
        assert_eq!(d_slot.known_lower_bound, k);
        assert!(!d_slot.unknown);

        // The compound slot envelope counts the SAME COMMs (whole-signature
        // attribution, Def 7.4 — the envelope's structure does not change Δ).
        let d_comp = demand(&par, &compound);
        assert_eq!(d_comp.known_lower_bound, d_slot.known_lower_bound);
        assert!(!d_comp.unknown);

        // The OSLF funds judgment applies to the slot exactly as to any signature:
        // resolvable demand ⇒ Def 19 `Σ_slot ≥ Δ_slot`, margin inert.
        assert!(is_funded(&d_slot, k, 0)); // Σ = Δ ⇒ funded
        assert!(is_funded(&d_slot, k + 5, 2)); // funded; margin inert for resolvable
        assert!(!is_funded(&d_slot, k - 1, 0)); // under-supplied ⇒ rejected
                                                // An ABSENT / empty slot pool (Σ = 0) with positive demand is rejected.
        assert!(!is_funded(&d_slot, 0, 0));

        // The slot is keyed via the same canonical `lane_hash` basis as any
        // ground signature, and distinct slots — and the compound — get distinct,
        // collision-free keys (the slot is unforgeable).
        assert_eq!(sig_key(&slot), slot.lane_hash());
        assert_ne!(sig_key(&slot), sig_key(&other_slot));
        assert_ne!(sig_key(&slot), sig_key(&compound));
    }
}

#[cfg(kani)]
mod kani_funding {
    //! D3 (DR-9) bounded model check of the per-signature funding/settlement
    //! NO-UNDERFLOW property (Commit 2 — replaces the retired
    //! `escrow = limit × price` kani). The settlement debit is the per-COMM
    //! demand `Δ_s`; an admitted (funded) deploy's debit must never underflow
    //! the supply pool Σ⟦s⟧ (`post = pre − Δ ≥ 0`).
    use super::*;

    #[kani::proof]
    fn funded_settlement_debit_never_underflows_supply() {
        let demand: i64 = kani::any();
        let supply: i64 = kani::any();
        let margin: i64 = kani::any();
        // Bound the inputs to a sane non-adversarial domain (the gate computes in
        // i128, but Δ / Σ / margin are bounded balances/counts in practice).
        kani::assume(demand >= 0 && demand <= 1_000_000);
        kani::assume(supply >= 0 && supply <= 1_000_000);
        kani::assume(margin >= 0 && margin <= 1_000_000);

        let analysis = DemandEntry {
            known_lower_bound: demand,
            unknown: false,
        };
        if is_funded(&analysis, supply, margin) {
            // Resolvable demand (`unknown == false`) ⇒ Def 19 `Σ ≥ Δ`, so the
            // settlement write `post = Σ − Δ` is non-negative (never underflows the
            // pool). The margin is inert here (F-B); the `≥ margin` headroom is
            // claimed only for UNKNOWN demand — see `unknown_demand_applies_margin`.
            assert!(supply - demand >= 0);
        }
    }

    #[kani::proof]
    fn resolvable_reject_below_demand() {
        let demand: i64 = kani::any();
        let supply: i64 = kani::any();
        let margin: i64 = kani::any();
        kani::assume(demand >= 0 && demand <= 1_000_000);
        kani::assume(supply >= 0 && supply <= 1_000_000);
        kani::assume(margin >= 0 && margin <= 1_000_000);

        let analysis = DemandEntry {
            known_lower_bound: demand,
            unknown: false,
        };
        // Resolvable demand: Def 19 reject direction — Σ strictly below Δ is NOT
        // funded, and the margin is inert (it does NOT raise the bar). F-B.
        if supply < demand {
            assert!(!is_funded(&analysis, supply, margin));
        }
    }

    #[kani::proof]
    fn unknown_demand_applies_margin() {
        let demand: i64 = kani::any();
        let supply: i64 = kani::any();
        let margin: i64 = kani::any();
        kani::assume(demand >= 0 && demand <= 1_000_000);
        kani::assume(supply >= 0 && supply <= 1_000_000);
        kani::assume(margin >= 0 && margin <= 1_000_000);

        let analysis = DemandEntry {
            known_lower_bound: demand,
            unknown: true,
        };
        // Over-approximated demand (Thm 20): funded ⇒ Σ ≥ Δ + margin (the safe
        // direction guarantees BOTH no-underflow AND the margin headroom), and
        // Σ strictly below Δ + margin is rejected.
        if is_funded(&analysis, supply, margin) {
            assert!(supply - demand >= margin);
            assert!(supply - demand >= 0);
        }
        if supply < demand + margin {
            assert!(!is_funded(&analysis, supply, margin));
        }
    }
}
