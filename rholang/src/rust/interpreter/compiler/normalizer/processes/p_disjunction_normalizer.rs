use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::{Connective, ConnectiveBody, Par};
use rholang_parser::ast::AnnProc;
use rholang_parser::SourceSpan;

use crate::rust::interpreter::compiler::exports::{FreeMap, ProcVisitInputs, ProcVisitOutputs};
use crate::rust::interpreter::compiler::normalize_drive::{NormKont, NormVal, NormWork, Step};
use crate::rust::interpreter::errors::InterpreterError;
use crate::rust::interpreter::util::prepend_connective;

/// `P \/ Q`, descend half.
///
/// ⚠ Unlike conjunction, **neither** disjunct sees the other's bindings: both
/// are normalized against `FreeMap::default()` and the enclosing `free_map` is
/// the one that gains the connective. The operands are still ordered — the left
/// one decides whether the result folds into an existing `ConnOrBody` — so the
/// machine still schedules them one at a time.
#[inline(never)]
pub(crate) fn descend_p_disjunction<'ast>(
    left: AnnProc<'ast>,
    right: AnnProc<'ast>,
    input: ProcVisitInputs,
) -> Step<'ast> {
    let bound_map_chain = input.bound_map_chain.clone();
    Step::Descend {
        kont: NormKont::Disjunction {
            right,
            input,
            span: SourceSpan {
                start: left.span.start,
                end: right.span.end,
            },
            left_par: None,
        },
        work: NormWork::Proc {
            proc: left,
            input: ProcVisitInputs {
                par: Par::default(),
                bound_map_chain,
                free_map: FreeMap::default(),
            },
        },
    }
}

/// `P \/ Q`, combine half.
/// `P disjunction Q` on a **fresh** drive. Only the unit tests enter here; the dispatch
/// pushes [`descend_p_disjunction`]'s `Step` onto the drive it is already
/// on, so the whole traversal stays one loop.
pub fn normalize_p_disjunction<'ast>(
    left: &'ast AnnProc<'ast>,
    right: &'ast AnnProc<'ast>,
    input: ProcVisitInputs,
    env: &std::collections::HashMap<String, Par>,
    parser: &'ast rholang_parser::RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    use crate::rust::interpreter::compiler::normalize_drive::norm_drive_from;
    let step = descend_p_disjunction(*left, *right, input);
    norm_drive_from(step, env, parser).map(NormVal::into_proc)
}

#[inline(never)]
pub(crate) fn combine_p_disjunction<'ast>(
    right: AnnProc<'ast>,
    input: ProcVisitInputs,
    span: SourceSpan,
    left_par: Option<Par>,
    value: NormVal,
) -> Result<Step<'ast>, InterpreterError> {
    let result = value.into_proc();
    let Some(lp) = left_par else {
        let bound_map_chain = input.bound_map_chain.clone();
        return Ok(Step::Descend {
            kont: NormKont::Disjunction {
                right,
                input,
                span,
                left_par: Some(result.par),
            },
            work: NormWork::Proc {
                proc: right,
                input: ProcVisitInputs {
                    par: Par::default(),
                    bound_map_chain,
                    free_map: FreeMap::default(),
                },
            },
        });
    };

    let result_connective = match lp.single_connective() {
        Some(Connective {
            connective_instance: Some(ConnectiveInstance::ConnOrBody(conn_body)),
        }) => Connective {
            connective_instance: Some(ConnectiveInstance::ConnOrBody(ConnectiveBody {
                ps: {
                    let mut ps = conn_body.ps.clone();
                    ps.push(result.par);
                    ps
                },
            })),
        },
        _ => Connective {
            connective_instance: Some(ConnectiveInstance::ConnOrBody(ConnectiveBody {
                ps: vec![lp, result.par],
            })),
        },
    };

    let result_par = prepend_connective(
        input.par.clone(),
        result_connective.clone(),
        input.bound_map_chain.depth() as i32,
    );

    let updated_free_map = input.free_map.add_connective(
        result_connective
            .connective_instance
            .expect("combine_p_disjunction: the connective was just constructed as `Some`"),
        span,
    );

    Ok(Step::Done(NormVal::Proc(ProcVisitOutputs {
        par: result_par,
        free_map: updated_free_map,
    })))
}


//rholang/src/test/scala/coop/rchain/rholang/interpreter/compiler/normalizer/ProcMatcherSpec.scala
#[cfg(test)]
mod tests {
    use models::rhoapi::connective::ConnectiveInstance;
    use models::rhoapi::{Connective, ConnectiveBody};
    use models::rust::utils::new_freevar_par;
    use pretty_assertions::assert_eq;

    use crate::rust::interpreter::test_utils::utils::proc_visit_inputs_and_env;

    #[test]
    fn p_disjunction_should_delegate_but_not_count_any_free_variables_inside() {
        use super::normalize_p_disjunction;
        use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

        let (inputs, env) = proc_visit_inputs_and_env();

        let parser = rholang_parser::RholangParser::new();

        let left_proc = ParBuilderUtil::create_ast_proc_var("x", &parser);
        let right_proc = ParBuilderUtil::create_ast_proc_var("x", &parser);

        let result =
            normalize_p_disjunction(&left_proc, &right_proc, inputs.clone(), &env, &parser);
        let expected_result = inputs
            .par
            .with_connectives(vec![Connective {
                connective_instance: Some(ConnectiveInstance::ConnOrBody(ConnectiveBody {
                    ps: vec![
                        new_freevar_par(0, Vec::new()),
                        new_freevar_par(0, Vec::new()),
                    ],
                })),
            }])
            .with_connective_used(true);

        assert_eq!(result.clone().unwrap().par, expected_result);
        assert_eq!(
            result.clone().unwrap().free_map.level_bindings,
            inputs.free_map.level_bindings
        );
        assert_eq!(
            result.clone().unwrap().free_map.next_level,
            inputs.free_map.next_level
        );
    }
}
