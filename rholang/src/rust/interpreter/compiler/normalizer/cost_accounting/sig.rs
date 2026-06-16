//! Signature resolution: surface [`Signature`] → [`Sig`] IR → native funding
//! [`accounting::Sig`](crate::rust::interpreter::accounting::Sig).
//!
//! W1 does NOT derive its own supply channel (the transpiler's Part-B
//! `supply_channel`/`atom_channel` are DROPPED): native `from_sig` is the
//! authoritative, consensus-anchored channel basis (design §3.1 — native wins).
//! Surface forms DECORATE; they never re-emit metered operations. So this module
//! only RESOLVES a surface signature to a native `Sig` (the `from_sig` of which is
//! `Σ⟦s⟧`); the located-stack attribution that uses it is a metering-context
//! concern (Phase 3), not codegen here.
//!
//! Resolution is **binding-sensitive** ("signatures are names"): a `new`/`for`-
//! bound ground sig is RING-FENCED (keyed on the binder's stable source span via
//! [`canon_bound`] → `ir::Sig::Bound`, bridged to a distinct native `Ground`),
//! while a FREE ground sig is content-by-spelling ([`canon_ground`]), so the same
//! free `g` is one global channel across deploys/parties (the §9 rendezvous).

use std::collections::HashMap;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;
use prost::Message;
use rholang_parser::ast::{AnnProc, Name, Signature, Var};
use rholang_parser::{RholangParser, SourceSpan};

use super::ir::Sig;
use crate::rust::interpreter::accounting::{Sig as NativeSig, SignatureChannel};
use crate::rust::interpreter::compiler::bound_map_chain::BoundMapChain;
use crate::rust::interpreter::compiler::normalize::{normalize_ann_proc, ProcVisitInputs, VarSort};
use crate::rust::interpreter::errors::InterpreterError;

/// Lower a surface [`Signature`] to the [`Sig`] IR.
///
/// * `Ground(g)` — `g` must be a bare identifier (v1); a wildcard `_` or a
///   quoted-principal `@P` ground sig is rejected (the latter is the bounded
///   future `@(P)` extension, symmetric with `#(P)`).
/// * `Hash(#P)` — the quote principal; `P` is canonicalized depth-independently.
/// * `Compound(s₁ * s₂)` — flattened + key-sorted via [`Sig::compound`].
/// * `Transfer(s₁ ⊸ s₂)` — the lollipop is *term-level* sugar
///   ([`super::desugar`]); it must never reach a fundable position, so it is
///   rejected here (a lollipop is a transfer capability, not a fundable atom).
pub fn signature_to_ir<'ast>(
    sig: &Signature<'ast>,
    bound_map_chain: &BoundMapChain<VarSort>,
    env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<Sig, InterpreterError> {
    match sig {
        Signature::Ground(name) => match name {
            // Binding-sensitive `Σ⟦g⟧`: a ground sig that resolves to a
            // `new`/`for`-binder is RING-FENCED (keyed on the binder's stable
            // identity); a FREE sig is content-by-spelling (one global channel).
            Name::NameVar(Var::Id(id)) => match bound_map_chain.get(id.name) {
                Some(ctx) => Ok(Sig::Bound(canon_bound(&ctx.source_span))),
                None => Ok(Sig::Ground(canon_ground(id.name))),
            },
            Name::NameVar(Var::Wildcard) => Err(InterpreterError::NormalizerError(
                "cost-accounting: a wildcard `_` is not a valid ground signature".to_string(),
            )),
            Name::Quote(_) => Err(InterpreterError::NormalizerError(
                "cost-accounting: a quoted-principal ground signature `@P` is not supported in v1 \
                 (use a section signature `# P` for code-hash principals)"
                    .to_string(),
            )),
        },
        Signature::Hash(proc) => Ok(Sig::Quote(canon_quote(proc, env, parser)?)),
        Signature::Compound(left, right) => {
            let left_ir = signature_to_ir(left, bound_map_chain, env, parser)?;
            let right_ir = signature_to_ir(right, bound_map_chain, env, parser)?;
            Ok(Sig::compound(vec![left_ir, right_ir]))
        }
        Signature::Transfer(_, _) => Err(InterpreterError::NormalizerError(
            "cost-accounting: a lollipop `-o` (transfer) signature is term-level sugar and must be \
             desugared before lowering; it cannot fund a term directly"
                .to_string(),
        )),
    }
}

/// Resolve a surface [`Signature`] straight to the native funding
/// [`accounting::Sig`](crate::rust::interpreter::accounting::Sig) — the
/// recognition + IR-bridge composite. Validates the sig (via
/// [`signature_to_ir`], which rejects a wildcard / `@P` ground / a bare lollipop
/// in fundable position) and bridges the IR to the consensus algebra via
/// [`Sig::to_native`]. The native `from_sig` of the result is the supply channel
/// `Σ⟦s⟧`; W1 NEVER derives a separate channel (design §3.1).
pub fn signature_to_native_sig<'ast>(
    sig: &Signature<'ast>,
    bound_map_chain: &BoundMapChain<VarSort>,
    env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<NativeSig, InterpreterError> {
    Ok(signature_to_ir(sig, bound_map_chain, env, parser)?.to_native())
}

/// Resolve a surface [`Signature`] to its native supply channel `Σ⟦s⟧` — the
/// consensus-anchored basis via [`SignatureChannel::from_sig`] (design §3.1:
/// native `from_sig` wins; surface forms DECORATE). This is the channel a
/// per-redex COMM is matched against for located-stack attribution (Phase 3,
/// P14). It is binding-sensitive through [`signature_to_native_sig`] (a
/// `new`-bound sig ring-fences to a `DOMAIN_BOUND`-keyed channel), so a free `g`
/// and a `new`-bound `g` derive DISTINCT channels, while two free `g` (any deploy)
/// derive the SAME channel — the §9 rendezvous.
pub fn signature_to_channel<'ast>(
    sig: &Signature<'ast>,
    bound_map_chain: &BoundMapChain<VarSort>,
    env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<Par, InterpreterError> {
    Ok(SignatureChannel::from_sig(&signature_to_native_sig(sig, bound_map_chain, env, parser)?).par)
}

/// Canonical bytes for a `new`-bound ground principal: the binder's source span,
/// a stable, binder-unique, binding-depth-INDEPENDENT identity. Two uses of the
/// same bound signature (e.g. a token and its gate, both in the binder's scope)
/// canonicalise to the same bytes — so they resolve to the same channel — while
/// two distinct `new`-binders never collide (each `new` has a distinct source
/// span), which is exactly the ring-fencing guarantee.
pub fn canon_bound(span: &SourceSpan) -> Vec<u8> {
    format!(
        "{}:{}-{}:{}",
        span.start.line, span.start.col, span.end.line, span.end.col
    )
    .into_bytes()
}

/// Canonical bytes for a ground principal: the wire encoding of the
/// sort-canonicalized `Par` holding the identifier as a ground string. Depends
/// only on the spelling, so the same `g` everywhere yields the same channel (and
/// the §9 rendezvous works).
pub fn canon_ground(name: &str) -> Vec<u8> {
    let par = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GString(name.to_string())),
    }]);
    ParSortMatcher::sort_match(&par).term.encode_to_vec()
}

/// Canonical bytes for a quote principal `#P`: the wire encoding of `𝒫⟦P⟧`,
/// normalized **standalone at de Bruijn depth 0** (a fresh [`ProcVisitInputs`]).
/// Normalizing at a fixed depth makes the encoding binder-depth-independent and
/// α-invariant — a `#P` that references an outer bound name hashes the same
/// wherever it appears. `FN_s(#P) = FN(P)`: free names are part of the principal's
/// identity (paper §3), so they are not rejected.
pub fn canon_quote<'ast>(
    proc: &AnnProc<'ast>,
    env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<Vec<u8>, InterpreterError> {
    let normalized = normalize_ann_proc(proc, ProcVisitInputs::new(), env, parser)?;
    Ok(ParSortMatcher::sort_match(&normalized.par)
        .term
        .encode_to_vec())
}
