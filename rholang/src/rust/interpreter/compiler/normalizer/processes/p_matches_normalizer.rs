use models::rhoapi::{expr, EMatches, Expr, Par};
use rholang_parser::ast::AnnProc;

use super::exports::InterpreterError;
use crate::rust::interpreter::compiler::exports::{FreeMap, ProcVisitInputs, ProcVisitOutputs};
use crate::rust::interpreter::compiler::normalize_drive::{NormKont, NormVal, NormWork, Step};
use crate::rust::interpreter::util::prepend_expr;

/// `P matches Q`, descend half — the target.
///
/// ⚠ The pattern is normalized one binding level **deeper**
/// (`bound_map_chain.push()`) and in a fresh free map, and the result keeps the
/// **target's** free map, not the pattern's: a `matches` pattern binds nothing
/// outside itself.
#[inline(never)]
pub(crate) fn descend_p_matches<'ast>(
    left: AnnProc<'ast>,
    right: AnnProc<'ast>,
    input: ProcVisitInputs,
) -> Step<'ast> {
    let bound_map_chain = input.bound_map_chain.clone();
    let free_map = input.free_map.clone();
    Step::Descend {
        kont: NormKont::Matches {
            right,
            input,
            left: None,
        },
        work: NormWork::Proc {
            proc: left,
            input: ProcVisitInputs {
                par: Par::default(),
                bound_map_chain,
                free_map,
            },
        },
    }
}

/// `P matches Q`, combine half.
/// `P matches Q` on a **fresh** drive. Only the unit tests enter here; the dispatch
/// pushes [`descend_p_matches`]'s `Step` onto the drive it is already
/// on, so the whole traversal stays one loop.
pub fn normalize_p_matches<'ast>(
    left: &'ast AnnProc<'ast>,
    right: &'ast AnnProc<'ast>,
    input: ProcVisitInputs,
    env: &std::collections::HashMap<String, Par>,
    parser: &'ast rholang_parser::RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    use crate::rust::interpreter::compiler::normalize_drive::norm_drive_from;
    let step = descend_p_matches(*left, *right, input);
    norm_drive_from(step, env, parser).map(NormVal::into_proc)
}

#[inline(never)]
pub(crate) fn combine_p_matches<'ast>(
    right: AnnProc<'ast>,
    input: ProcVisitInputs,
    left: Option<ProcVisitOutputs>,
    value: NormVal,
) -> Result<Step<'ast>, InterpreterError> {
    let result = value.into_proc();
    let Some(left_result) = left else {
        return Ok(Step::Descend {
            kont: NormKont::Matches {
                right,
                input: input.clone(),
                left: Some(result),
            },
            work: NormWork::Proc {
                proc: right,
                input: ProcVisitInputs {
                    par: Par::default(),
                    bound_map_chain: input.bound_map_chain.push(),
                    free_map: FreeMap::default(),
                },
            },
        });
    };

    let new_expr = Expr {
        expr_instance: Some(expr::ExprInstance::EMatchesBody(EMatches {
            target: Some(left_result.par),
            pattern: Some(result.par),
        })),
    };

    let prepend_par = prepend_expr(input.par, new_expr, input.bound_map_chain.depth() as i32);

    Ok(Step::Done(NormVal::Proc(ProcVisitOutputs {
        par: prepend_par,
        free_map: left_result.free_map,
    })))
}


//rholang/src/test/scala/coop/rchain/rholang/interpreter/compiler/normalizer/ProcMatcherSpec.scala
#[cfg(test)]
mod tests {
    use models::rhoapi::connective::ConnectiveInstance::ConnNotBody;
    use models::rhoapi::{expr, Connective, EMatches, Expr, Par};
    use models::rust::utils::{new_gint_par, new_wildcard_par};
    use pretty_assertions::assert_eq;

    use crate::rust::interpreter::test_utils::utils::proc_visit_inputs_and_env;
    use crate::rust::interpreter::util::prepend_expr;

    #[test]
    fn p_matches_should_normalize_one_matches_wildcard() {
        // Test: 1 matches _
        use rholang_parser::ast::Var;

        use super::normalize_p_matches;
        use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

        let (inputs, env) = proc_visit_inputs_and_env();
        let parser = rholang_parser::RholangParser::new();

        // Create "1 matches _" - LongLiteral matches Wildcard
        let left_proc = ParBuilderUtil::create_ast_long_literal(1, &parser);
        let right_proc = ParBuilderUtil::create_ast_proc_var_from_var(Var::Wildcard, &parser);

        let result = normalize_p_matches(&left_proc, &right_proc, inputs.clone(), &env, &parser);

        let expected_par = prepend_expr(
            inputs.par.clone(),
            Expr {
                expr_instance: Some(expr::ExprInstance::EMatchesBody(EMatches {
                    target: Some(new_gint_par(1, Vec::new(), false)),
                    pattern: Some(new_wildcard_par(Vec::new(), true)),
                })),
            },
            0,
        );

        assert_eq!(result.clone().unwrap().par, expected_par);
        assert_eq!(result.unwrap().par.connective_used, false);
    }

    #[test]
    fn p_matches_should_normalize_correctly_one_matches_two() {
        // Test: 1 matches 2
        use super::normalize_p_matches;
        use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

        let (inputs, env) = proc_visit_inputs_and_env();
        let parser = rholang_parser::RholangParser::new();

        // Create "1 matches 2" - LongLiteral matches LongLiteral
        let left_proc = ParBuilderUtil::create_ast_long_literal(1, &parser);
        let right_proc = ParBuilderUtil::create_ast_long_literal(2, &parser);

        let result = normalize_p_matches(&left_proc, &right_proc, inputs.clone(), &env, &parser);

        let expected_par = prepend_expr(
            inputs.par.clone(),
            Expr {
                expr_instance: Some(expr::ExprInstance::EMatchesBody(EMatches {
                    target: Some(new_gint_par(1, Vec::new(), false)),
                    pattern: Some(new_gint_par(2, Vec::new(), false)),
                })),
            },
            0,
        );

        assert_eq!(result.clone().unwrap().par, expected_par);
        assert_eq!(result.unwrap().par.connective_used, false);
    }

    #[test]
    fn p_matches_should_normalize_one_matches_tilda_with_connective_used_false() {
        // Test: 1 matches ~1
        use rholang_parser::ast::UnaryExpOp;

        use super::normalize_p_matches;
        use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

        let (inputs, env) = proc_visit_inputs_and_env();
        let parser = rholang_parser::RholangParser::new();

        // Create "1 matches ~1" - LongLiteral matches (Negation LongLiteral)
        let left_proc = ParBuilderUtil::create_ast_long_literal(1, &parser);
        let arg = ParBuilderUtil::create_ast_long_literal(1, &parser);
        let right_proc = ParBuilderUtil::create_ast_unary_exp(UnaryExpOp::Negation, arg, &parser);

        let result = normalize_p_matches(&left_proc, &right_proc, inputs.clone(), &env, &parser);

        let expected_par = prepend_expr(
            inputs.par.clone(),
            Expr {
                expr_instance: Some(expr::ExprInstance::EMatchesBody(EMatches {
                    target: Some(new_gint_par(1, Vec::new(), false)),
                    pattern: Some(Par {
                        connectives: vec![Connective {
                            connective_instance: Some(ConnNotBody(new_gint_par(
                                1,
                                Vec::new(),
                                false,
                            ))),
                        }],
                        connective_used: true,
                        ..Par::default().clone()
                    }),
                })),
            },
            0,
        );

        assert_eq!(result.clone().unwrap().par, expected_par);
        assert_eq!(result.unwrap().par.connective_used, false);
    }

    #[test]
    fn p_matches_should_normalize_tilda_one_matches_one_with_connective_used_true() {
        // Test: ~1 matches 1
        use rholang_parser::ast::UnaryExpOp;

        use super::normalize_p_matches;
        use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

        let (inputs, env) = proc_visit_inputs_and_env();
        let parser = rholang_parser::RholangParser::new();

        // Create "~1 matches 1" - (Negation LongLiteral) matches LongLiteral
        let arg = ParBuilderUtil::create_ast_long_literal(1, &parser);
        let left_proc = ParBuilderUtil::create_ast_unary_exp(UnaryExpOp::Negation, arg, &parser);
        let right_proc = ParBuilderUtil::create_ast_long_literal(1, &parser);

        let result = normalize_p_matches(&left_proc, &right_proc, inputs.clone(), &env, &parser);

        let expected_par = prepend_expr(
            inputs.par.clone(),
            Expr {
                expr_instance: Some(expr::ExprInstance::EMatchesBody(EMatches {
                    target: Some(Par {
                        connectives: vec![Connective {
                            connective_instance: Some(ConnNotBody(new_gint_par(
                                1,
                                Vec::new(),
                                false,
                            ))),
                        }],
                        connective_used: true,
                        ..Par::default().clone()
                    }),
                    pattern: Some(new_gint_par(1, Vec::new(), false)),
                })),
            },
            0,
        );

        assert_eq!(result.clone().unwrap().par, expected_par);
        assert_eq!(result.unwrap().par.connective_used, true)
    }
}
