//! Cost-accounting transpiler: lower cost-accounted Rholang surface syntax
//! (Greg `app:concrete`) — signed terms `{% P %}[s]`, bare token stacks
//! `s :: … :: ()`, signatures `g ∣ #P ∣ s∘s` (+ the uniform and lollipop `⊸`
//! sugars, and per-clause signed binds `{% y<-x %}[s]`) — to stable Rholang
//! (`Par`) via the cost-accounted-rho §8 source-to-source translation.
//!
//! This is the *internalise ⊣ include* adjunction of the cost endofunctor
//! (continued-gslt-cost-v2 §9) realized as a normalizer pass — a stopgap until
//! the native *install ⊣ forget* reducer (`../f1r3node-rust/`) lands. Both
//! adjunctions implement the same OSLF/GSLT funding port, so the swap is an
//! interface-level change, not a rewrite. See `docs/cost-accounting/transpiler.md`.
//!
//! ## Seam (Split Phase · Protected Variations · Hexagonal Ports-&-Adapters)
//!
//! [`CostLoweringStrategy`] is the single port; [`strategy`] the single
//! construction site; [`lower::LowerToPar`] the one adapter (the *internalise*
//! functor). `compiler::normalize` dispatches through the port only — the
//! Strangler-Fig seam that lets the lowering be retired when native lands.
//!
//! ## Translation pieces (catamorphism over the cost AST)
//!
//! * [`sig`] — `Σ⟦s⟧` content-addressed supply channel, `N`/canonicalization;
//! * [`token`] — `K⟦S⟧` token-stack send chain;
//! * [`signed_term`] — `T⟦{P}_s⟧` fuel gate (`P⟦·⟧` = `normalize_ann_proc`) and
//!   the per-clause join (`signed_term::lower_signed_join`);
//! * [`desugar`] — uniform-signing + lollipop term rewrites + signed-bind strip;
//! * [`ir`] — the [`ir::Sig`] IR + [`ir::ResourceSignature`] funding-algebra
//!   mirror.
//!
//! ## Forward integration (Anti-Corruption Layer + Dependency Inversion)
//!
//! [`ir::Sig`] mirrors the *shape* of f1r3node's native `accounting::Sig` /
//! `ResourceSignature` with NO hard dependency, so the signature/funding
//! algebra is reused across the swap (and by a future MeTTaIL adapter — DR-24).
//! The interaction-cut components are delineated as [`InteractionCut`] but kept
//! concrete-Rholang-only for v1 (avoid Speculative Generality).

pub mod desugar;
pub mod infra;
pub mod ir;
pub mod lower;
pub mod oslf;
pub mod pattern_guard;
pub mod sig;
pub mod signed_term;
pub mod token;

#[cfg(test)]
mod tests;

use std::collections::HashMap;

use models::rhoapi::Par;
use rholang_parser::ast::{AnnProc, Signature, TokenStack};
use rholang_parser::RholangParser;

use crate::rust::interpreter::compiler::normalize::{ProcVisitInputs, ProcVisitOutputs};
use crate::rust::interpreter::errors::InterpreterError;

/// The cost-lowering port: the §8 internalisation of a cost construct into a
/// stable `Par`. Symmetric and phase-stable — no `&mut` collector, because
/// Phase 1 needs no program-level pass (every `Σ⟦s⟧` is content-addressed
/// inline). `&'ast` throughout: the lowering reads borrowed AST and the
/// arena-allocated desugar products, both living for `'ast`.
pub trait CostLoweringStrategy<'ast> {
    /// `T⟦{P}_s⟧` — lower a signed term to its fuel gate(s).
    fn lower_signed_term(
        &self,
        inner: &'ast AnnProc<'ast>,
        sig: &'ast Signature<'ast>,
        input: ProcVisitInputs,
        env: &HashMap<String, Par>,
        parser: &'ast RholangParser<'ast>,
    ) -> Result<ProcVisitOutputs, InterpreterError>;

    /// `K⟦S⟧` — lower a bare token stack to its token send-chain (Greg
    /// `app:concrete` `Stk`; no `purse(...)` wrapper, no located identity —
    /// ring-fencing is via `new`-bound signatures, the binding-sensitive `Σ⟦s⟧`).
    fn lower_token_stack(
        &self,
        stack: &'ast TokenStack<'ast>,
        input: ProcVisitInputs,
        env: &HashMap<String, Par>,
        parser: &'ast RholangParser<'ast>,
    ) -> Result<ProcVisitOutputs, InterpreterError>;
}

/// The single strategy construction site (Protected Variations). Swapping the
/// lowering — or retiring it for the native reducer — touches only here.
pub fn strategy<'ast>() -> impl CostLoweringStrategy<'ast> { lower::LowerToPar }

/// The interaction-cut interface the cost endofunctor is parametric over
/// (cost-accounted-rho §3.x): the host calculus's contact/cut structure. The
/// §8 lowering in this module is the *internalise functor over this cut*; v1
/// implements only the Rholang instance ([`RhoInteractionCut`]). MeTTaIL
/// becomes a second instance once its backend migrates to the Rho machine
/// (DR-24) — at which point the lowering is reused parametrically rather than
/// rewritten. Kept deliberately minimal (no premature multi-language framework).
pub trait InteractionCut {
    /// The continuation sort `K` (for Rholang: `Par`).
    type Continuation;

    /// Name of the program-introduction former (`for` — surface = channel,
    /// continuation = body).
    const PROGRAM_INTRO: &'static str;

    /// Name of the environment-introduction former (`send` — surface =
    /// channel, continuation = payload).
    const ENV_INTRO: &'static str;
}

/// The Rholang instance of the interaction cut: `K = Par`,
/// `program_intro = for`, `env_intro = send`, `cut = comm`.
pub struct RhoInteractionCut;

impl InteractionCut for RhoInteractionCut {
    type Continuation = Par;
    const PROGRAM_INTRO: &'static str = "for";
    const ENV_INTRO: &'static str = "send";
}
