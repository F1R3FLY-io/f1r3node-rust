use std::collections::HashMap;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{ELambda, Expr, Par};
use rholang_parser::ast::{AnnProc, Id};
use rholang_parser::SourcePos;
use smallvec::SmallVec;

use crate::rust::interpreter::compiler::exports::{
    IdContextPos, ProcVisitInputs, ProcVisitOutputs,
};
use crate::rust::interpreter::compiler::normalize::{normalize_ann_proc, VarSort};
use crate::rust::interpreter::errors::InterpreterError;
use crate::rust::interpreter::matcher::has_locally_free::HasLocallyFree;
use crate::rust::interpreter::util::{filter_and_adjust_bitset, prepend_expr};

/// Normalize a lambda abstraction `λ(x_0: A_0?, …, x_{n-1}: A_{n-1}?). body`
/// into an `ELambda` expression. The surface order binds `x_i` to
/// BoundVar(i) inside the body (same convention as Phase 1a Expr-level
/// tests, lambda_expr_spec).
///
/// Phase 2b: type annotations on params are accepted syntactically but
/// discarded here — the Phase 2c typechecker will pick them up by widening
/// `ELambda` with a `paramTypes` field. For now, typed and untyped
/// lambdas produce identical proto representations.
pub fn normalize_p_lambda<'ast>(
    params: &SmallVec<[(Id<'ast>, Option<AnnProc<'ast>>); 1]>,
    body: &'ast AnnProc<'ast>,
    input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast rholang_parser::RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    let bindings: Vec<IdContextPos<VarSort>> = params
        .iter()
        .map(|(id, _ty)| {
            (id.name.to_string(), VarSort::ProcSort, SourcePos {
                line: id.pos.line,
                col: id.pos.col,
            })
        })
        .collect();

    let scope = input.bound_map_chain.put_all_pos(bindings);
    let bind_count = scope.get_count() - input.bound_map_chain.get_count();

    let body_result = normalize_ann_proc(
        body,
        ProcVisitInputs {
            par: Par::default(),
            bound_map_chain: scope,
            free_map: input.free_map.clone(),
        },
        env,
        parser,
    )?;

    let input_depth = input.bound_map_chain.depth() as i32;
    let body_par = body_result.par.clone();
    let locally_free = filter_and_adjust_bitset(body_par.locally_free(input_depth), bind_count);
    let connective_used = body_par.connective_used();

    let lambda = Expr {
        expr_instance: Some(ExprInstance::ELambdaBody(ELambda {
            bind_count: bind_count as i32,
            body: Some(body_par),
            locally_free,
            connective_used,
        })),
    };

    Ok(ProcVisitOutputs {
        par: prepend_expr(input.par, lambda, input_depth),
        free_map: body_result.free_map,
    })
}
