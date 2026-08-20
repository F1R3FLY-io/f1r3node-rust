use models::rust::par_construction::ParConstruction;
use rholang_parser::ast::Proc;

use super::exports::*;
use crate::rust::interpreter::compiler::exports::{
    ProcVisitInputs as PublicProcVisitInputs, ProcVisitOutputs as PublicProcVisitOutputs,
};
use crate::rust::interpreter::compiler::normalize::append_planned_expr;
use crate::rust::interpreter::compiler::normalize_drive::{ProcVisitInputs, ProcVisitOutputs};
use crate::rust::interpreter::compiler::normalizer::ground_normalize_matcher::normalize_ground;
use crate::rust::interpreter::matcher::has_locally_free::{
    expr_connective_used_ref, expr_locally_free_ref,
};
use crate::rust::interpreter::util::prepend_expr;

pub fn normalize_p_ground<'ast>(
    proc: &Proc<'ast>,
    input: PublicProcVisitInputs,
) -> Result<PublicProcVisitOutputs, InterpreterError> {
    normalize_ground(proc).map(|expr| PublicProcVisitOutputs {
        par: prepend_expr(input.par, expr, input.bound_map_chain.depth() as i32),
        free_map: input.free_map,
    })
}

pub(crate) fn plan_p_ground<'ast>(
    proc: &Proc<'ast>,
    input: ProcVisitInputs,
    construction: &mut ParConstruction,
) -> Result<ProcVisitOutputs, InterpreterError> {
    let expr = normalize_ground(proc)?;
    let locally_free = expr_locally_free_ref(&expr, input.bound_map_chain.depth() as i32);
    let connective_used = expr_connective_used_ref(&expr);
    Ok(ProcVisitOutputs {
        par: append_planned_expr(
            construction,
            input.par,
            &expr,
            &[],
            locally_free,
            connective_used,
        )?,
        free_map: input.free_map,
    })
}
