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
/// Param type annotations are optional per-slot: if any param carries
/// an annotation, `ELambda.paramTypes` is populated with length
/// `bind_count` and `Par::default()` in any slot the source left
/// un-annotated. If no params are annotated, `paramTypes` stays empty
/// (legacy shape). Annotations are normalized in the OUTER scope (no
/// binder is in scope while normalizing them), matching Pi behavior.
/// The reducer ignores the field; the typechecker cross-verifies
/// non-empty slots against the expected Pi's param types.
pub fn normalize_p_lambda<'ast>(
    params: &SmallVec<[(Id<'ast>, Option<AnnProc<'ast>>); 1]>,
    body: &'ast AnnProc<'ast>,
    input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast rholang_parser::RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    let input_bound_chain = input.bound_map_chain.clone();
    let input_depth = input.bound_map_chain.depth() as i32;

    // Normalize annotations (if any) in the outer scope, threading
    // `free_map` so each annotation's free variables accumulate.
    let any_annotated = params.iter().any(|(_, ty)| ty.is_some());
    let mut free_map = input.free_map.clone();
    let param_type_pars: Vec<Par> = if any_annotated {
        let mut out = Vec::with_capacity(params.len());
        for (_id, maybe_ty) in params.iter() {
            match maybe_ty {
                Some(ty) => {
                    let ty_result = normalize_ann_proc(
                        ty,
                        ProcVisitInputs {
                            par: Par::default(),
                            bound_map_chain: input_bound_chain.clone(),
                            free_map: free_map.clone(),
                        },
                        env,
                        parser,
                    )?;
                    free_map = ty_result.free_map;
                    out.push(ty_result.par);
                }
                None => out.push(Par::default()),
            }
        }
        out
    } else {
        Vec::new()
    };

    let bindings: Vec<IdContextPos<VarSort>> = params
        .iter()
        .map(|(id, _ty)| {
            (id.name.to_string(), VarSort::ProcSort, SourcePos {
                line: id.pos.line,
                col: id.pos.col,
            })
        })
        .collect();

    let scope = input_bound_chain.put_all_pos(bindings);
    let bind_count = scope.get_count() - input_bound_chain.get_count();

    let body_result = normalize_ann_proc(
        body,
        ProcVisitInputs {
            par: Par::default(),
            bound_map_chain: scope,
            free_map,
        },
        env,
        parser,
    )?;

    let body_par = body_result.par.clone();
    let body_locally_free = filter_and_adjust_bitset(body_par.locally_free(input_depth), bind_count);
    // Annotations contribute their own locally_free (outer scope).
    let mut locally_free = body_locally_free;
    for pt in &param_type_pars {
        locally_free = models::rust::utils::union(locally_free, pt.locally_free(input_depth));
    }
    let connective_used =
        body_par.connective_used() || param_type_pars.iter().any(|p| p.connective_used());

    let lambda = Expr {
        expr_instance: Some(ExprInstance::ELambdaBody(ELambda {
            bind_count: bind_count as i32,
            body: Some(body_par),
            locally_free,
            connective_used,
            param_types: param_type_pars,
        })),
    };

    Ok(ProcVisitOutputs {
        par: prepend_expr(input.par, lambda, input_depth),
        free_map: body_result.free_map,
    })
}
