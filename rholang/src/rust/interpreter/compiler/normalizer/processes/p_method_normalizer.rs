use models::rhoapi::{expr, EMethod, Expr, Par};
use models::rust::utils::union;
use rholang_parser::ast::{AnnProc, Id};

use crate::rust::interpreter::compiler::exports::{ProcVisitInputs, ProcVisitOutputs};
use crate::rust::interpreter::compiler::normalize_drive::{
    MethodK, NormKont, NormVal, NormWork, Step,
};
use crate::rust::interpreter::errors::InterpreterError;
use crate::rust::interpreter::util::prepend_expr;

/// `P.m(a, b, …)`, descend half — the RECEIVER first.
///
/// ⚠ **The arguments are then visited right-to-left.** The recursive form was
/// `args.iter().rev().try_fold(init, …)` with each result `insert(0, …)`-ed, so
/// the emitted `arguments` vector is in source order while the *free map*
/// threads from the last argument to the first. That is observable — it decides
/// which occurrence of a repeated free name gets the lower de Bruijn level — so
/// the machine reproduces both halves: [`arg_at`] indexes in reverse and
/// [`MethodK::acc_args`] is still built with `insert(0, …)`.
#[inline(never)]
pub(crate) fn descend_p_method<'ast>(
    receiver: AnnProc<'ast>,
    name_id: &'ast Id<'ast>,
    args: &'ast rholang_parser::ast::ProcList<'ast>,
    input: ProcVisitInputs,
) -> Step<'ast> {
    let input_depth = input.bound_map_chain.depth() as i32;
    let bound_map_chain = input.bound_map_chain.clone();
    let free_map = input.free_map.clone();
    Step::Descend {
        kont: NormKont::Method(Box::new(MethodK {
            args,
            idx: 0,
            method_name: name_id.name.to_string(),
            target: None,
            acc_args: Vec::with_capacity(args.len()),
            acc_locally_free: Vec::new(),
            acc_connective_used: false,
            free_map,
            bound_map_chain,
            input_par: input.par.clone(),
            input_depth,
        })),
        work: NormWork::Proc {
            proc: receiver,
            input: ProcVisitInputs {
                par: Par::default(),
                ..input
            },
        },
    }
}

/// Argument `i` **in reverse source order** — the `i`-th one the recursive
/// `args.iter().rev()` fold would have visited.
#[inline]
fn arg_at<'ast>(
    args: &'ast rholang_parser::ast::ProcList<'ast>,
    i: usize,
) -> Option<AnnProc<'ast>> {
    if i >= args.len() {
        return None;
    }
    Some(args[args.len() - 1 - i])
}

/// `P.m(a, b, …)`, combine half.
#[inline(never)]
pub(crate) fn combine_p_method<'ast>(
    mut k: Box<MethodK<'ast>>,
    value: NormVal,
) -> Result<Step<'ast>, InterpreterError> {
    let result = value.into_proc();
    if k.idx == 0 {
        // The RECEIVER has just finished.
        k.free_map = result.free_map;
        k.target = Some(result.par);
    } else {
        // Argument `idx - 1` (in reverse source order) has just finished.
        // ★ Leg-1: shallow fields first, then MOVE.
        let child_locally_free = result.par.locally_free.clone();
        let child_connective_used = result.par.connective_used;
        k.acc_args.insert(0, result.par);
        k.free_map = result.free_map;
        k.acc_locally_free = union(std::mem::take(&mut k.acc_locally_free), child_locally_free);
        k.acc_connective_used = k.acc_connective_used || child_connective_used;
    }

    if let Some(next) = arg_at(k.args, k.idx) {
        let child_input = ProcVisitInputs {
            par: Par::default(),
            bound_map_chain: k.bound_map_chain.clone(),
            free_map: k.free_map.clone(),
        };
        k.idx += 1;
        return Ok(Step::Descend {
            kont: NormKont::Method(k),
            work: NormWork::Proc {
                proc: next,
                input: child_input,
            },
        });
    }

    let MethodK {
        method_name,
        target,
        acc_args,
        acc_locally_free,
        acc_connective_used,
        free_map,
        input_par,
        input_depth,
        ..
    } = *k;
    let target = target.expect("combine_p_method: the receiver slot is filled before any argument");

    // ★ Leg-1 at the reader — see `p_send_normalizer`.
    let target_locally_free = target.locally_free.clone();
    let target_connective_used = target.connective_used;
    let method = EMethod {
        method_name,
        target: Some(target),
        arguments: acc_args,
        locally_free: union(target_locally_free, acc_locally_free),
        connective_used: target_connective_used || acc_connective_used,
    };

    let updated_par = prepend_expr(
        input_par,
        Expr {
            expr_instance: Some(expr::ExprInstance::EMethodBody(method)),
        },
        input_depth,
    );

    Ok(Step::Done(NormVal::Proc(ProcVisitOutputs {
        par: updated_par,
        free_map,
    })))
}

// See rholang/src/test/scala/coop/rchain/rholang/interpreter/compiler/normalizer/ProcMatcherSpec.scala
#[cfg(test)]
mod tests {
    use models::create_bit_vector;
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::{EMethod, Expr, Par};
    use models::rust::utils::{new_boundvar_par, new_gint_par};

    use crate::rust::interpreter::compiler::normalize::VarSort;
    use crate::rust::interpreter::test_utils::utils::proc_visit_inputs_and_env;
    use crate::rust::interpreter::util::prepend_expr;

    #[test]
    fn p_method_should_produce_proper_method_call() {
        use rholang_parser::ast::{Id, Var};
        use rholang_parser::SourcePos;

        use crate::rust::interpreter::compiler::normalize::normalize_ann_proc;
        use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

        let methods = vec![String::from("nth"), String::from("toByteArray")];

        fn test(method_name: String) {
            let parser = rholang_parser::RholangParser::new();
            let (mut inputs, env) = proc_visit_inputs_and_env();
            inputs.bound_map_chain =
                inputs
                    .bound_map_chain
                    .put_pos(("x".to_string(), VarSort::ProcSort, SourcePos {
                        line: 0,
                        col: 0,
                    }));

            // Create receiver: x (ProcVar)
            let receiver = ParBuilderUtil::create_ast_proc_var_from_var(
                Var::Id(Id {
                    name: "x",
                    pos: SourcePos { line: 0, col: 0 },
                }),
                &parser,
            );

            // Create method name
            let method_id = Id {
                name: &method_name,
                pos: SourcePos { line: 0, col: 0 },
            };

            // Create args: [0]
            let arg = ParBuilderUtil::create_ast_long_literal(0, &parser);

            // Create method call
            let method_call =
                ParBuilderUtil::create_ast_method(method_id, receiver, vec![arg], &parser);

            let result = normalize_ann_proc(&method_call, inputs.clone(), &env, &parser);
            assert!(result.is_ok());

            let expected_result = prepend_expr(
                Par::default(),
                Expr {
                    expr_instance: Some(ExprInstance::EMethodBody(EMethod {
                        method_name,
                        target: Some(new_boundvar_par(0, create_bit_vector(&vec![0]), false)),
                        arguments: vec![new_gint_par(0, Vec::new(), false)],
                        locally_free: create_bit_vector(&vec![0]),
                        connective_used: false,
                    })),
                },
                0,
            );

            assert_eq!(result.clone().unwrap().par, expected_result);
            assert_eq!(result.unwrap().free_map, inputs.free_map);
        }

        test(methods[0].clone());
        test(methods[1].clone());
    }
}
