//! Signature lowering: surface [`Signature`] → [`Sig`] IR → supply channel
//! `Σ⟦s⟧` (the `N`/`Σ` translation of cost-accounted-rho §8).
//!
//! `Σ⟦s⟧` is a **global, content-addressed, unforgeable** channel — a closed
//! `Par` (empty `locally_free`) identical at every use site, needing no
//! `bound_map_chain` resolution. Because it is global and content-addressed,
//! the *same* explicit signature shared by two principals is an intended
//! rendezvous (the §9 fee-interceptor pattern); cross-deploy isolation rests
//! on deploys being signed by different signatures, not on per-deploy
//! channels (verified against migration §5.8.1). See the Cost model in
//! `docs/cost-accounting/transpiler.md`.

use std::collections::HashMap;

use crypto::rust::hash::blake2b256::Blake2b256;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{Expr, GPrivate, GUnforgeable, Par};
use models::rust::rholang::implicits::concatenate_pars;
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;
use prost::Message;
use rholang_parser::ast::{AnnProc, Name, Signature, Var};
use rholang_parser::{RholangParser, SourceSpan};

use super::ir::{Sig, DOMAIN_BOUND, DOMAIN_GROUND, DOMAIN_QUOTE};
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
            // Binding-sensitive `Σ⟦g⟧` (Greg "signatures are names"): a ground sig
            // that resolves to a `new`/`for`-binder is RING-FENCED — keyed on the
            // binder's stable identity, so a fresh `new`-bound sig is unforgeable
            // and never aliases a free sig of the same identifier. A FREE sig is
            // content-by-spelling, so the *same* free `g` is one global channel
            // across deploys/parties (the §9 shared-rendezvous pattern).
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

/// Canonical bytes for a `new`-bound ground principal: the binder's source span,
/// a stable, binder-unique, binding-depth-INDEPENDENT identity. Two uses of the
/// same bound signature (e.g. a token and its gate, both in the binder's scope)
/// canonicalise to the same bytes — so they mint/consume on the same channel —
/// while two distinct `new`-binders never collide (each `new` has a distinct
/// source span), which is exactly the ring-fencing guarantee.
pub fn canon_bound(span: &SourceSpan) -> Vec<u8> {
    format!(
        "{}:{}-{}:{}",
        span.start.line, span.start.col, span.end.line, span.end.col
    )
    .into_bytes()
}

/// Canonical bytes for a ground principal: the wire encoding of the
/// sort-canonicalized `Par` holding the identifier as a ground string.
/// Depends only on the spelling, so the same `g` everywhere yields the same
/// channel (and the §9 rendezvous works).
pub fn canon_ground(name: &str) -> Vec<u8> {
    let par = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GString(name.to_string())),
    }]);
    ParSortMatcher::sort_match(&par).term.encode_to_vec()
}

/// Canonical bytes for a quote principal `#P`: the wire encoding of `𝒫⟦P⟧`,
/// normalized **standalone at de Bruijn depth 0** (a fresh
/// [`ProcVisitInputs`]). Normalizing at a fixed depth makes the encoding
/// binder-depth-independent and α-invariant — a `#P` that references an outer
/// bound name hashes the same wherever it appears (the depth-sensitive
/// ambient normal form would not). `FN_s(#P) = FN(P)`: free names are part of
/// the principal's identity (paper §3), so they are not rejected.
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

/// The supply channel `Σ⟦s⟧`: a closed, content-addressed `Par`.
///
/// * atom ⇒ `@(GUnforgeable(GPrivate(Blake2b256(DOMAIN ‖ content))))` —
///   `GPrivate` makes it unforgeable (user code cannot mint a token on it),
///   the domain separator keeps the ground/quote axes from aliasing;
/// * compound ⇒ the canonical-sorted parallel composition of the component
///   channels (`ParSortMatcher::sort_match` makes it permutation-invariant, so
///   `Σ⟦(a*b)*c⟧ = Σ⟦c*b*a⟧`), matching native `from_sig`'s `And` arm.
pub fn supply_channel(sig: &Sig) -> Par {
    match sig {
        Sig::Ground(content) => atom_channel(DOMAIN_GROUND, content),
        Sig::Bound(content) => atom_channel(DOMAIN_BOUND, content),
        Sig::Quote(content) => atom_channel(DOMAIN_QUOTE, content),
        Sig::Compound(components) => {
            let mut combined = Par::default();
            for component in components {
                combined = concatenate_pars(combined, supply_channel(component));
            }
            ParSortMatcher::sort_match(&combined).term
        }
    }
}

/// Convenience: lower a surface signature straight to its supply channel.
pub fn signature_channel<'ast>(
    sig: &Signature<'ast>,
    bound_map_chain: &BoundMapChain<VarSort>,
    env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<Par, InterpreterError> {
    Ok(supply_channel(&signature_to_ir(
        sig,
        bound_map_chain,
        env,
        parser,
    )?))
}

fn atom_channel(domain: &[u8], content: &[u8]) -> Par {
    let mut preimage = Vec::with_capacity(domain.len() + content.len());
    preimage.extend_from_slice(domain);
    preimage.extend_from_slice(content);
    let id = Blake2b256::hash(preimage);
    Par::default().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id })),
    }])
}
