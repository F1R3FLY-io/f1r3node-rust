//! The normalizer arm of the Reified RSpaces `use s { P }` block.

use std::collections::HashMap;

use models::rhoapi::Par;
use models::rust::reified::builders::{push_process_expr, use_block};
use rholang_parser::ast::{AnnProc, Name};
use rholang_parser::RholangParser;

use crate::rust::interpreter::compiler::exports::{
    NameVisitInputs, ProcVisitInputs, ProcVisitOutputs,
};
use crate::rust::interpreter::compiler::normalize::normalize_ann_proc;
use crate::rust::interpreter::compiler::normalizer::name_normalize_matcher::normalize_name;
use crate::rust::interpreter::errors::InterpreterError;

/// Normalizes `use space { body }` to the USE tuple `(USE, s, P)` in `Par.exprs`. The space is
/// a name and the body a process, both in the current scope, the space first.
pub fn normalize_p_use_block<'ast>(
    space: &Name<'ast>,
    body: &AnnProc<'ast>,
    input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    let space_result = normalize_name(
        space,
        NameVisitInputs {
            bound_map_chain: input.bound_map_chain.clone(),
            free_map: input.free_map,
        },
        env,
        parser,
    )?;
    let body_result = normalize_ann_proc(
        body,
        ProcVisitInputs {
            par: Par::default(),
            bound_map_chain: input.bound_map_chain,
            free_map: space_result.free_map,
        },
        env,
        parser,
    )?;
    Ok(ProcVisitOutputs {
        par: push_process_expr(input.par, use_block(space_result.par, body_result.par)),
        free_map: body_result.free_map,
    })
}
