use std::collections::HashMap;

use models::rhoapi::cost_signature::Value as CostSignatureValue;
use models::rhoapi::{CostSignature, CostSignatureCompound, CostSignedTerm, CostStack, Par};
use models::rust::rholang::sorter::cost_accounting_sorter::sort_signature;
use models::rust::utils::union;
use rholang_parser::ast::{AnnProc, Receipts, Signature, TokenStack};
use rholang_parser::RholangParser;

use super::desugar;
use super::sig::{signature_to_wire, wire_signature_locally_free};
use crate::rust::interpreter::compiler::normalize::{
    normalize_ann_proc, ProcVisitInputs, ProcVisitOutputs,
};
use crate::rust::interpreter::errors::InterpreterError;

pub fn recognize_signed_term<'ast>(
    inner: &'ast AnnProc<'ast>,
    sig: &'ast Signature<'ast>,
    input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    let (core_inner, core_sig): (AnnProc<'ast>, &Signature<'ast>) = match sig {
        Signature::Transfer(s1, s2) => (desugar::lollipop(*inner, s2, parser)?, &**s1),
        core => (desugar::uniform_sign(*inner, core, parser), core),
    };
    let (signature, free_map) = signature_to_wire(
        core_sig,
        &input.bound_map_chain,
        input.free_map.clone(),
        env,
        parser,
    )?;
    let normalized = normalize_ann_proc(
        &core_inner,
        ProcVisitInputs {
            par: Par::default(),
            bound_map_chain: input.bound_map_chain.clone(),
            free_map,
        },
        env,
        parser,
    )?;
    let mut par = input.par;
    par.locally_free = union(
        par.locally_free,
        union(
            normalized.par.locally_free.clone(),
            wire_signature_locally_free(&signature),
        ),
    );
    par.connective_used = par.connective_used || normalized.par.connective_used;
    let (body, signature) = flatten_nested_signature(normalized.par, signature);
    par.cost_signed_terms.push(CostSignedTerm {
        body: Some(body),
        signature: Some(signature),
        // Added by G3 (DR-135, S3b): a signed term of the surface syntax has
        // no held price. Only the reducer makes held terms.
        held_price: None,
    });
    Ok(ProcVisitOutputs {
        par,
        free_map: normalized.free_map,
    })
}

/// Added by G3 (DR-135): the signed term that is all of `par`. It is `Some`
/// only when `par` holds exactly one signed term and no other process. The
/// normalizer flattens such a nested signature, and the owner checks of a
/// fired body read the signature of such a body (A5).
pub(crate) fn solely_signed(par: &Par) -> Option<&CostSignedTerm> {
    match par.cost_signed_terms.as_slice() {
        [term]
            if par.sends.is_empty()
                && par.receives.is_empty()
                && par.news.is_empty()
                && par.exprs.is_empty()
                && par.matches.is_empty()
                && par.unforgeables.is_empty()
                && par.bundles.is_empty()
                && par.connectives.is_empty()
                && par.conditionals.is_empty()
                && par.cost_stacks.is_empty() =>
        {
            Some(term)
        }
        _ => None,
    }
}

fn flatten_nested_signature(par: Par, signature: CostSignature) -> (Par, CostSignature) {
    // Changed by G3 (DR-135): the test moved to `solely_signed`.
    // let solely_nested = par.sends.is_empty()
    //     && par.receives.is_empty()
    //     && par.news.is_empty()
    //     && par.exprs.is_empty()
    //     && par.matches.is_empty()
    //     && par.unforgeables.is_empty()
    //     && par.bundles.is_empty()
    //     && par.connectives.is_empty()
    //     && par.conditionals.is_empty()
    //     && par.cost_stacks.is_empty()
    //     && par.cost_signed_terms.len() == 1;
    let solely_nested = solely_signed(&par).is_some();
    if !solely_nested {
        return (par, signature);
    }
    let nested = par.cost_signed_terms.into_iter().next().unwrap();
    let nested_signature = nested.signature.unwrap();
    let compound = sort_signature(&CostSignature {
        value: Some(CostSignatureValue::Compound(CostSignatureCompound {
            elements: vec![signature, nested_signature],
        })),
    })
    .term;
    (nested.body.unwrap(), compound)
}

pub fn recognize_token_stack<'ast>(
    stack: &'ast TokenStack<'ast>,
    input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    let mut free_map = input.free_map;
    let mut cells = Vec::with_capacity(stack.layers.len());
    let mut locally_free = Vec::new();
    for layer in &stack.layers {
        let (signature, next_free_map) =
            signature_to_wire(layer, &input.bound_map_chain, free_map, env, parser)?;
        locally_free = union(locally_free, wire_signature_locally_free(&signature));
        cells.push(signature);
        free_map = next_free_map;
    }
    let mut par = input.par;
    par.locally_free = union(par.locally_free, locally_free);
    if !cells.is_empty() {
        par.cost_stacks.push(CostStack { cells });
    }
    Ok(ProcVisitOutputs { par, free_map })
}

pub fn recognize_signed_join<'ast>(
    receipts: &'ast Receipts<'ast>,
    body: &'ast AnnProc<'ast>,
    input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    crate::rust::interpreter::compiler::normalizer::processes::p_input_normalizer::normalize_p_input(
        receipts, body, input, env, parser,
    )
}
