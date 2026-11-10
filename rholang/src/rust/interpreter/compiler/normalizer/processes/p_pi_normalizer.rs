use std::collections::HashMap;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EPi, Expr, Par};
use models::rust::utils::union;
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

/// Normalize a Pi type `(x_0: A_0, …, x_{n-1}: A_{n-1}) -> B` into an
/// `EPi` expression. Param types live in the outer scope; the codomain
/// `B` is normalized under a `bound_map_chain` extended with the param
/// identifiers. CoC Phase 2a wires Pi through the pipeline without a
/// typechecker: a Pi flows as an opaque type-level value, reducible to
/// itself under WHNF.
pub fn normalize_p_pi<'ast>(
    params: &SmallVec<[(Id<'ast>, AnnProc<'ast>); 1]>,
    codomain: &'ast AnnProc<'ast>,
    input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast rholang_parser::RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    let input_par = input.par.clone();
    let input_depth = input.bound_map_chain.depth() as i32;
    let input_bound_chain = input.bound_map_chain.clone();

    // Normalize each parameter type in the OUTER scope (no binder yet),
    // threading `free_map` so each type's free variables accumulate.
    let mut free_map = input.free_map.clone();
    let mut param_type_pars: Vec<Par> = Vec::with_capacity(params.len());
    for (_id, ty) in params.iter() {
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
        param_type_pars.push(ty_result.par);
    }

    // Build the extended scope: push each param name as a ProcSort binding.
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

    let codomain_result = normalize_ann_proc(
        codomain,
        ProcVisitInputs {
            par: Par::default(),
            bound_map_chain: scope,
            free_map,
        },
        env,
        parser,
    )?;

    let codomain_par = codomain_result.par.clone();

    // locally_free of the Pi = union of param-type free vars (outer scope)
    // with codomain free vars filtered to drop the binders.
    let codomain_filtered =
        filter_and_adjust_bitset(codomain_par.locally_free(input_depth), bind_count);
    let mut locally_free = codomain_filtered;
    for p in &param_type_pars {
        locally_free = union(locally_free, p.locally_free(input_depth));
    }

    let connective_used =
        codomain_par.connective_used() || param_type_pars.iter().any(|p| p.connective_used());

    let pi = Expr {
        expr_instance: Some(ExprInstance::EPiBody(EPi {
            bind_count: bind_count as i32,
            param_types: param_type_pars,
            codomain: Some(codomain_par),
            locally_free,
            connective_used,
        })),
    };

    Ok(ProcVisitOutputs {
        par: prepend_expr(input_par, pi, input_depth),
        free_map: codomain_result.free_map,
    })
}
