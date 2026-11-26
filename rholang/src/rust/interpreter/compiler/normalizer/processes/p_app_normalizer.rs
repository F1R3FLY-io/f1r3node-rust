use std::collections::HashMap;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EApp, Expr, Par};
use models::rust::utils::union;
use rholang_parser::ast::{AnnProc, ProcList};

use crate::rust::interpreter::compiler::exports::{ProcVisitInputs, ProcVisitOutputs};
use crate::rust::interpreter::compiler::normalize::normalize_ann_proc;
use crate::rust::interpreter::errors::InterpreterError;
use crate::rust::interpreter::matcher::has_locally_free::HasLocallyFree;
use crate::rust::interpreter::util::prepend_expr;

/// Normalize a function application `fun(arg_0, …, arg_{n-1})` into an
/// `EApp` expression. β-reduction happens at reduce time (`reduce.rs`); the
/// normalizer only builds the Expr tree.
///
/// Arguments threaded left-to-right through `free_map`, matching the
/// convention used by `Method` / `Send` normalization.
pub fn normalize_p_app<'ast>(
    fun: &'ast AnnProc<'ast>,
    args: &ProcList<'ast>,
    input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast rholang_parser::RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    let input_par = input.par.clone();
    let input_depth = input.bound_map_chain.depth() as i32;

    let fun_result = normalize_ann_proc(
        fun,
        ProcVisitInputs {
            par: Par::default(),
            bound_map_chain: input.bound_map_chain.clone(),
            free_map: input.free_map.clone(),
        },
        env,
        parser,
    )?;

    let mut free_map = fun_result.free_map.clone();
    let mut arg_pars: Vec<Par> = Vec::with_capacity(args.len());
    for arg in args.iter() {
        let arg_result = normalize_ann_proc(
            arg,
            ProcVisitInputs {
                par: Par::default(),
                bound_map_chain: input.bound_map_chain.clone(),
                free_map: free_map.clone(),
            },
            env,
            parser,
        )?;
        free_map = arg_result.free_map;
        arg_pars.push(arg_result.par);
    }

    let fun_par = fun_result.par;

    let mut locally_free = fun_par.locally_free(input_depth);
    let mut connective_used = fun_par.connective_used();
    for arg in &arg_pars {
        locally_free = union(locally_free, arg.locally_free(input_depth));
        connective_used |= arg.connective_used();
    }

    let app = Expr {
        expr_instance: Some(ExprInstance::EAppBody(EApp {
            fun: Some(fun_par),
            args: arg_pars,
            locally_free,
            connective_used,
        })),
    };

    Ok(ProcVisitOutputs {
        par: prepend_expr(input_par, app, input_depth),
        free_map,
    })
}
