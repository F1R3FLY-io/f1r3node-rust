use std::collections::HashMap;

use models::rhoapi::{Foreach, Par};
use models::rust::utils::union;
use rholang_parser::ast::{AnnProc, Id};
use rholang_parser::SourcePos;

use crate::rust::interpreter::compiler::exports::{
    IdContextPos, ProcVisitInputs, ProcVisitOutputs,
};
use crate::rust::interpreter::compiler::normalize::{normalize_ann_proc, VarSort};
use crate::rust::interpreter::errors::InterpreterError;
use crate::rust::interpreter::matcher::has_locally_free::HasLocallyFree;
use crate::rust::interpreter::util::{filter_and_adjust_bitset, prepend_foreach};

/// f1r3lang-with-monads Stage B.2 native-eval path.
///
/// Normalize `foreach ( binder <- source ) yield ( yield_name ) proc`
/// into a `Foreach` process-level node on `par.foreaches`. The source
/// and yield_name are normalized in the OUTER scope (binder not yet
/// visible); the body is normalized in a scope extended with the
/// binder, so inside the body `binder` resolves to `BoundVar(0)` (the
/// same convention as `p_lambda_normalizer` / `p_pi_normalizer`
/// post-`e09be002c`: single param pushes in reverse into one binder).
///
/// Reducer-side native evaluation lands separately; this file only
/// shapes the Par. The §4.7 reference translation (concurrent
/// decomposition via new/for/match/send) is explicitly not emitted.
pub fn normalize_p_foreach<'ast>(
    binder: &Id<'ast>,
    source: &'ast AnnProc<'ast>,
    yield_name: &Id<'ast>,
    body: &'ast AnnProc<'ast>,
    input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast rholang_parser::RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    let input_par = input.par.clone();
    let input_bound_chain = input.bound_map_chain.clone();
    let input_depth = input.bound_map_chain.depth() as i32;

    // 1. Normalize source in the outer scope.
    let source_result = normalize_ann_proc(
        source,
        ProcVisitInputs {
            par: Par::default(),
            bound_map_chain: input_bound_chain.clone(),
            free_map: input.free_map.clone(),
        },
        env,
        parser,
    )?;

    // 2. Normalize yield_name in the outer scope. The yield_name is a
    //    channel identifier; we lower it to a quoted-name Par via the
    //    same ProcVar path the lambda normalizer uses for its binders'
    //    references.
    let yield_name_par = {
        use models::rhoapi::expr::ExprInstance;
        use models::rhoapi::var::VarInstance;
        use models::rhoapi::{EVar, Expr, Var};
        // Look up the yield_name as a name in the surrounding scope.
        // If it's a fresh identifier not yet bound, we leave it as a
        // free-var reference; downstream the reducer will either match
        // it against an outer `new` or fail at emit time.
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

    // 3. Extend the scope with the binder for body normalization.
    let binder_binding: Vec<IdContextPos<VarSort>> = vec![(
        binder.name.to_string(),
        VarSort::ProcSort,
        SourcePos {
            line: binder.pos.line,
            col: binder.pos.col,
        },
    )];
    let scope = input_bound_chain.put_all_pos(binder_binding);
    let bind_count = scope.get_count() - input_bound_chain.get_count();

    // 4. Normalize body under the extended scope.
    let body_result = normalize_ann_proc(
        body,
        ProcVisitInputs {
            par: Par::default(),
            bound_map_chain: scope,
            free_map: source_result.free_map.clone(),
        },
        env,
        parser,
    )?;

    // 5. Compose locally_free: source + yield_name (outer scope) plus
    //    body's free vars with the binder filtered out.
    let body_par = body_result.par.clone();
    let body_free_shifted =
        filter_and_adjust_bitset(body_par.locally_free(input_depth), bind_count);
    let mut locally_free = body_free_shifted;
    locally_free = union(locally_free, source_result.par.locally_free(input_depth));
    locally_free = union(locally_free, yield_name_par.locally_free(input_depth));

    let connective_used = source_result.par.connective_used()
        || yield_name_par.connective_used()
        || body_par.connective_used();

    let foreach = Foreach {
        bind_count: bind_count as i32,
        source: Some(source_result.par),
        body: Some(body_par),
        yield_name: Some(yield_name_par),
        locally_free,
        connective_used,
    };

    Ok(ProcVisitOutputs {
        par: prepend_foreach(input_par, foreach),
        free_map: body_result.free_map,
    })
}
