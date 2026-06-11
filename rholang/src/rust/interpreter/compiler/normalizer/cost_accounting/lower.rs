//! `LowerToPar` — the *internalise* adapter behind the cost port. Implements
//! [`CostLoweringStrategy`] by delegating to the §8 translations (`T` for
//! signed terms, `K` for purses). This is the Strangler-Fig seam: when the
//! native reducer lands, this adapter is retired (native does no lowering)
//! while the signature/funding algebra ([`super::ir`]) is reused.

use std::collections::HashMap;

use models::rhoapi::Par;
use models::rust::rholang::implicits::concatenate_pars;
use rholang_parser::ast::{AnnProc, Signature, TokenStack};
use rholang_parser::RholangParser;

use super::{signed_term, token, CostLoweringStrategy};
use crate::rust::interpreter::compiler::normalize::{ProcVisitInputs, ProcVisitOutputs};
use crate::rust::interpreter::errors::InterpreterError;

/// The Phase-1 cost-lowering strategy: signed terms → fuel gates, purses →
/// token send-chains.
pub struct LowerToPar;

impl<'ast> CostLoweringStrategy<'ast> for LowerToPar {
    fn lower_signed_term(
        &self,
        inner: &'ast AnnProc<'ast>,
        sig: &'ast Signature<'ast>,
        input: ProcVisitInputs,
        env: &HashMap<String, Par>,
        parser: &'ast RholangParser<'ast>,
    ) -> Result<ProcVisitOutputs, InterpreterError> {
        signed_term::lower_signed_term(*inner, sig, input, env, parser)
    }

    fn lower_token_stack(
        &self,
        stack: &'ast TokenStack<'ast>,
        input: ProcVisitInputs,
        env: &HashMap<String, Par>,
        parser: &'ast RholangParser<'ast>,
    ) -> Result<ProcVisitOutputs, InterpreterError> {
        // `K⟦S⟧` runs in parallel with the rest. A bare stack carries no located
        // identity — ring-fencing is via `new`-bound signatures (the
        // binding-sensitive `Σ⟦s⟧`, resolved in this scope's `bound_map_chain`).
        let tokens = token::lower_token_stack(&stack.layers, &input.bound_map_chain, env, parser)?;
        Ok(ProcVisitOutputs {
            par: concatenate_pars(input.par, tokens),
            free_map: input.free_map,
        })
    }
}
