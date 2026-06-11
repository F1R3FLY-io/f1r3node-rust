//! Token-stack lowering `K⟦·⟧`.
//!
//! A bare token stack `() ∣ s :: S` (Greg `app:concrete` `Stk`; no `purse(...)`
//! wrapper) lowers to a right-folded chain of sends on the supply channels:
//! `K⟦()⟧ = Nil`, `K⟦s :: S⟧ = Σ⟦s⟧ ! ( K⟦S⟧ )`. A token is therefore a *send*
//! that a fuel-gate (`super::signed_term`) consumes: one COMM binds the payload
//! (the remaining balance `K⟦S⟧`), which `*t` re-releases — so one token funds
//! one gated process and the rest of the stack flows on (per-COMM metering =
//! worktree rule R1).
//!
//! A **combined-cell** layer (a `Compound` signature, e.g. `a (*) b :: ()`)
//! mints its token on the compound channel `Σ⟦a∘b⟧`; since the gates listen on
//! the components, a [`super::infra`] splitter is emitted alongside to bridge
//! it to the chained split form (R3/R5). **Ring-fencing** is binding-sensitive:
//! a layer whose signature is `new`-bound mints on the binder-keyed
//! `Σ⟦bound s⟧` (over `DOMAIN_BOUND`), reachable only by a gate in the same
//! scope; a free signature mints on the shared `Σ⟦free s⟧` (over
//! `DOMAIN_GROUND`). This replaces the located purse (see the NOTE below).

use std::collections::HashMap;

use models::rhoapi::Par;
use models::rust::rholang::implicits::concatenate_pars;
use models::rust::utils::new_send_par;
use rholang_parser::ast::Signature;
use rholang_parser::RholangParser;

use super::infra::build_splitter;
use super::ir::Sig;
use super::sig::{signature_to_ir, supply_channel};
use crate::rust::interpreter::compiler::bound_map_chain::BoundMapChain;
use crate::rust::interpreter::compiler::normalize::VarSort;
use crate::rust::interpreter::errors::InterpreterError;

/// Lower a token stack to its send chain, plus a splitter for every combined-cell
/// (`Compound`) layer. Binding-sensitive: a layer whose signature is `new`-bound
/// mints on the ring-fenced `Σ⟦s⟧` (keyed on the binder), so the fuel is reachable
/// only by a gate in the same scope; a free signature mints on the global
/// content-addressed `Σ⟦s⟧`.
pub fn lower_token_stack<'ast>(
    layers: &[Signature<'ast>],
    bound_map_chain: &BoundMapChain<VarSort>,
    env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<Par, InterpreterError> {
    // Right fold from the `()` tail (`Nil`), head last.
    let mut acc = Par::default();
    let mut splitters = Par::default();
    for layer in layers.iter().rev() {
        let sig = signature_to_ir(layer, bound_map_chain, env, parser)?;
        let channel = supply_channel(&sig);
        acc = new_send_par(
            channel,
            vec![acc],
            false,
            Vec::new(),
            false,
            Vec::new(),
            false,
        );
        if matches!(sig, Sig::Compound(_)) {
            splitters = concatenate_pars(splitters, build_splitter(&sig));
        }
    }
    Ok(concatenate_pars(acc, splitters))
}

// NOTE: the located-purse lowering (`Σ_I⟦s⟧ = @(*I ∣ *Σ⟦s⟧)`, surface
// `purse(I, …)`) was removed: Greg's concrete syntax has no located form, and
// ring-fencing is now realised by `new`-bound signatures via the
// binding-sensitive `Σ⟦s⟧` (see `super::sig::signature_to_ir` + `Sig::Bound`).
