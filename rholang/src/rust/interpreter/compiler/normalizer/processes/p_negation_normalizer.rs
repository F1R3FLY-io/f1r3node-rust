use models::rhoapi::{connective, Connective, Par};
use rholang_parser::ast::AnnProc;

use crate::rust::interpreter::compiler::exports::{FreeMap, ProcVisitInputs, ProcVisitOutputs};
use crate::rust::interpreter::compiler::normalize_drive::{NormKont, NormVal, NormWork, Step};
use crate::rust::interpreter::errors::InterpreterError;
use crate::rust::interpreter::util::prepend_connective;

/// `~P`, descend half.
///
/// The body is normalized in a **fresh** free map (`FreeMap::default()`) — a
/// negation's bindings do not escape it — while the enclosing `free_map` is
/// carried across in the continuation and gains the connective at the end.
#[inline(never)]
pub(crate) fn descend_p_negation<'ast>(
    arg: AnnProc<'ast>,
    unary_expr_span: rholang_parser::SourceSpan,
    input: ProcVisitInputs,
) -> Step<'ast> {
    // Use the actual span of the entire UnaryExp (~<expr>) for accurate source location
    let ann_proc = AnnProc {
        proc: arg.proc,
        span: unary_expr_span,
    };
    let bound_map_chain = input.bound_map_chain.clone();
    Step::Descend {
        kont: NormKont::Negation {
            input,
            span: unary_expr_span,
        },
        work: NormWork::Proc {
            proc: ann_proc,
            input: ProcVisitInputs {
                par: Par::default(),
                bound_map_chain,
                free_map: FreeMap::default(),
            },
        },
    }
}

/// `~P` on a **fresh** drive. Only the unit tests enter here.
pub fn normalize_p_negation<'ast>(
    arg: &'ast rholang_parser::ast::Proc<'ast>,
    unary_expr_span: rholang_parser::SourceSpan,
    input: ProcVisitInputs,
    env: &std::collections::HashMap<String, Par>,
    parser: &'ast rholang_parser::RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    use crate::rust::interpreter::compiler::normalize_drive::{norm_drive_from, NormVal};
    let step = descend_p_negation(
        AnnProc {
            proc: arg,
            span: unary_expr_span,
        },
        unary_expr_span,
        input,
    );
    norm_drive_from(step, env, parser).map(NormVal::into_proc)
}

/// `~P`, combine half.
#[inline(never)]
pub(crate) fn combine_p_negation<'ast>(
    input: ProcVisitInputs,
    unary_expr_span: rholang_parser::SourceSpan,
    value: NormVal,
) -> Result<Step<'ast>, InterpreterError> {
    let body_result = value.into_proc();

    // Create Connective with ConnNotBody
    let connective = Connective {
        connective_instance: Some(connective::ConnectiveInstance::ConnNotBody(body_result.par)),
    };

    let updated_par = prepend_connective(
        input.par,
        connective.clone(),
        input.bound_map_chain.depth() as i32,
    );

    Ok(Step::Done(NormVal::Proc(ProcVisitOutputs {
        par: updated_par,
        free_map: input.free_map.add_connective(
            connective
                .connective_instance
                .expect("combine_p_negation: the connective was just constructed as `Some`"),
            unary_expr_span, // Use the actual span of the entire negation operation
        ),
    })))
}


//rholang/src/test/scala/coop/rchain/rholang/interpreter/compiler/normalizer/ProcMatcherSpec.scala
#[cfg(test)]
mod tests {
    use models::rhoapi::connective::ConnectiveInstance;
    use models::rhoapi::Connective;
    use models::rust::utils::new_freevar_par;
    use pretty_assertions::assert_eq;

    use crate::rust::interpreter::test_utils::utils::proc_visit_inputs_and_env;

    #[test]
    fn p_negation_should_delegate_but_not_count_any_free_variables_inside() {
        use rholang_parser::ast::{Id, Proc, Var};
        use rholang_parser::{SourcePos, SourceSpan};

        use super::normalize_p_negation;

        let (inputs, env) = proc_visit_inputs_and_env();
        let parser = rholang_parser::RholangParser::new();
        let var_proc = Proc::ProcVar(Var::Id(Id {
            name: "x",
            pos: SourcePos { line: 1, col: 1 },
        }));

        let test_span = SourceSpan {
            start: SourcePos { line: 1, col: 1 },
            end: SourcePos { line: 1, col: 2 },
        };
        let result = normalize_p_negation(&var_proc, test_span, inputs.clone(), &env, &parser);
        let expected_result = inputs
            .par
            .with_connectives(vec![Connective {
                connective_instance: Some(ConnectiveInstance::ConnNotBody(new_freevar_par(
                    0,
                    Vec::new(),
                ))),
            }])
            .with_connective_used(true);

        assert_eq!(result.clone().unwrap().par, expected_result);
        assert_eq!(
            result.clone().unwrap().free_map.level_bindings,
            inputs.free_map.level_bindings
        );
        assert_eq!(
            result.unwrap().free_map.next_level,
            inputs.free_map.next_level
        )
    }
}
