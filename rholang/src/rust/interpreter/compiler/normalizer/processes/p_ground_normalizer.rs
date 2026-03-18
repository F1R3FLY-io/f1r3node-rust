use rholang_parser::ast::Proc;

use super::exports::*;
use crate::rust::interpreter::compiler::exports::{ProcVisitInputs, ProcVisitOutputs};
use crate::rust::interpreter::compiler::normalizer::ground_normalize_matcher::normalize_ground;
use crate::rust::interpreter::util::prepend_expr;

pub fn normalize_p_ground<'ast>(
    proc: &Proc<'ast>,
    input: ProcVisitInputs,
) -> Result<ProcVisitOutputs, InterpreterError> {
    normalize_ground(proc).map(|expr| {
        let new_par = prepend_expr(
            input.par.clone(),
            expr,
            input.bound_map_chain.depth() as i32,
        );
        ProcVisitOutputs {
            par: new_par,
            free_map: input.free_map.clone(),
        }
    })
}
