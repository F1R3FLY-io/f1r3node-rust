use std::collections::HashMap;

use models::rhoapi::{Foreach, Par};
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
use crate::rust::interpreter::util::{filter_and_adjust_bitset, prepend_foreach};

/// f1r3lang-with-monads Stage B.2 native-eval path (§4.4 `&` + §4.5
/// `where`).
///
/// Normalize
///   `foreach ( b_0 <- m_0 & … & b_{n-1} <- m_{n-1} where c? )
///    yield ( yield_name ) proc`
/// into a `Foreach` process-level node on `par.foreaches`.
///
/// Sources are normalized in the OUTER scope (binds within a group
/// are *independent* per §4.4 — no source may mention the other's
/// binder). The optional `where` guard and the body are normalized
/// in a scope extended with all binders in reverse source order, so
/// `b_i` resolves to `BoundVar(i)` inside the body (same convention
/// as `p_lambda_normalizer` / `p_pi_normalizer` after e09be002c).
pub fn normalize_p_foreach<'ast>(
    binds: &SmallVec<[(Id<'ast>, AnnProc<'ast>); 1]>,
    where_cond: Option<&'ast AnnProc<'ast>>,
    yield_name: &Id<'ast>,
    body: &'ast AnnProc<'ast>,
    input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast rholang_parser::RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    let input_par = input.par.clone();
    let input_bound_chain = input.bound_map_chain.clone();
    let input_depth = input.bound_map_chain.depth() as i32;

    // 1. Normalize each source in the outer scope. Thread free_map
    //    across sources so a free var captured by source_0 is reused
    //    by source_1, etc.
    let mut free_map = input.free_map.clone();
    let mut source_pars: Vec<Par> = Vec::with_capacity(binds.len());
    for (_id, source_ast) in binds.iter() {
        let result = normalize_ann_proc(
            source_ast,
            ProcVisitInputs {
                par: Par::default(),
                bound_map_chain: input_bound_chain.clone(),
                free_map: free_map.clone(),
            },
            env,
            parser,
        )?;
        free_map = result.free_map;
        source_pars.push(result.par);
    }

    // 2. Normalize yield_name in the outer scope. Must already be
    //    bound (via `new r` or similar).
    let yield_name_par = {
        use models::rhoapi::expr::ExprInstance;
        use models::rhoapi::var::VarInstance;
        use models::rhoapi::{EVar, Expr, Var};
        match input_bound_chain.get(yield_name.name) {
            Some(bound) => Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::EVarBody(EVar {
                    v: Some(Var {
                        var_instance: Some(VarInstance::BoundVar(bound.index as i32)),
                    }),
                })),
            }]),
            None => {
                return Err(InterpreterError::NormalizerError(format!(
                    "foreach yield-name `{}` is not bound in the surrounding \
                     scope; introduce it with `new {} in {{ ... }}` first",
                    yield_name.name, yield_name.name
                )));
            }
        }
    };

    // 3. Extend the scope with all binders. Push in REVERSE source
    //    order so `b_0` lands at the lowest BoundVar index inside
    //    the body (matching the lambda / Pi convention from
    //    e09be002c for `x_i -> BoundVar(i)`).
    let binder_bindings: Vec<IdContextPos<VarSort>> = binds
        .iter()
        .rev()
        .map(|(id, _src)| {
            (id.name.to_string(), VarSort::ProcSort, SourcePos {
                line: id.pos.line,
                col: id.pos.col,
            })
        })
        .collect();
    let scope = input_bound_chain.put_all_pos(binder_bindings);
    let bind_count = scope.get_count() - input_bound_chain.get_count();

    // 4. Normalize optional `where` guard under the extended scope.
    let where_par = if let Some(cond_ast) = where_cond {
        let r = normalize_ann_proc(
            cond_ast,
            ProcVisitInputs {
                par: Par::default(),
                bound_map_chain: scope.clone(),
                free_map: free_map.clone(),
            },
            env,
            parser,
        )?;
        free_map = r.free_map;
        Some(r.par)
    } else {
        None
    };

    // 5. Normalize body under the extended scope.
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

    // 6. Compose locally_free. Body + where contribute free vars with
    //    the binders filtered out; sources + yield_name contribute
    //    their outer-scope free vars directly.
    let body_free = filter_and_adjust_bitset(body_par.locally_free(input_depth), bind_count);
    let where_free = where_par
        .as_ref()
        .map(|p| filter_and_adjust_bitset(p.locally_free(input_depth), bind_count))
        .unwrap_or_default();
    let mut locally_free = union(body_free, where_free);
    for s in &source_pars {
        locally_free = union(locally_free, s.locally_free(input_depth));
    }
    locally_free = union(locally_free, yield_name_par.locally_free(input_depth));

    let connective_used = source_pars.iter().any(|p| p.connective_used())
        || yield_name_par.connective_used()
        || where_par.as_ref().is_some_and(|p| p.connective_used())
        || body_par.connective_used();

    let foreach = Foreach {
        bind_count: bind_count as i32,
        sources: source_pars,
        body: Some(body_par),
        yield_name: Some(yield_name_par),
        where_cond: where_par,
        locally_free,
        connective_used,
    };

    Ok(ProcVisitOutputs {
        par: prepend_foreach(input_par, foreach),
        free_map: body_result.free_map,
    })
}
