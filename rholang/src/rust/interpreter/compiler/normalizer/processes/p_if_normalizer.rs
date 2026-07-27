use models::rhoapi::{If, Par};
use models::rust::utils::union;
use rholang_parser::ast::AnnProc;

use crate::rust::interpreter::compiler::bound_map_chain::BoundMapChain;
use crate::rust::interpreter::compiler::exports::{ProcVisitInputs, ProcVisitOutputs};
use crate::rust::interpreter::compiler::normalize::VarSort;
use crate::rust::interpreter::compiler::normalize_drive::{
    IfPhase, NormKont, NormVal, NormWork, Step,
};
use crate::rust::interpreter::errors::InterpreterError;

/// `if (C) T else F`, descend half — the CONDITION.
///
/// ★ Two asymmetries are fused in here, both from `normalize.rs`'s
/// `IfThenElse` arm rather than from `normalize_p_if` itself:
///
/// 1. the whole conditional is normalized against an **empty** `par`, and the
///    caller's `par` is `append`ed to the *result* — so the entry `par` travels
///    in the continuation as `outer_par` and is not visible to any child;
/// 2. a missing `else` is not skipped, it is normalized as a **synthesised
///    `Nil`** at span `0:0-0:0`. That is a no-op on the term but it is *not* a
///    no-op on the free map plumbing, so it is reproduced literally rather than
///    short-circuited.
#[inline(never)]
pub(crate) fn descend_p_if<'ast>(
    condition: AnnProc<'ast>,
    if_true: AnnProc<'ast>,
    if_false: Option<AnnProc<'ast>>,
    input: ProcVisitInputs,
) -> Step<'ast> {
    // Follow same pattern as original IfElse: use empty Par for normalization,
    // then append original Par
    let outer_par = input.par;
    let bound_map_chain = input.bound_map_chain;
    Step::Descend {
        kont: NormKont::If {
            if_true,
            if_false,
            bound_map_chain: bound_map_chain.clone(),
            outer_par,
            phase: IfPhase::Condition,
            condition: None,
            true_case: None,
        },
        work: NormWork::Proc {
            proc: condition,
            input: ProcVisitInputs {
                par: Par::default(),
                bound_map_chain,
                free_map: input.free_map,
            },
        },
    }
}

/// `if (C) T else F`, combine half.
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn combine_p_if<'ast>(
    if_true: AnnProc<'ast>,
    if_false: Option<AnnProc<'ast>>,
    bound_map_chain: BoundMapChain<VarSort>,
    outer_par: Par,
    phase: IfPhase,
    condition: Option<ProcVisitOutputs>,
    true_case: Option<ProcVisitOutputs>,
    value: NormVal,
    parser: &'ast rholang_parser::RholangParser<'ast>,
) -> Result<Step<'ast>, InterpreterError> {
    let result = value.into_proc();
    match phase {
        IfPhase::Condition => Ok(Step::Descend {
            kont: NormKont::If {
                if_true,
                if_false,
                bound_map_chain: bound_map_chain.clone(),
                outer_par,
                phase: IfPhase::TrueCase,
                condition: Some(result.clone()),
                true_case: None,
            },
            work: NormWork::Proc {
                proc: if_true,
                input: ProcVisitInputs {
                    par: Par::default(),
                    bound_map_chain,
                    free_map: result.free_map,
                },
            },
        }),

        IfPhase::TrueCase => {
            let false_proc = match if_false {
                Some(p) => p,
                None => {
                    let nil_proc_ref = parser.ast_builder().const_nil();
                    rholang_parser::ast::AnnProc {
                        proc: nil_proc_ref,
                        span: rholang_parser::SourceSpan {
                            start: rholang_parser::SourcePos { line: 0, col: 0 },
                            end: rholang_parser::SourcePos { line: 0, col: 0 },
                        },
                    }
                }
            };
            Ok(Step::Descend {
                kont: NormKont::If {
                    if_true,
                    if_false,
                    bound_map_chain: bound_map_chain.clone(),
                    outer_par,
                    phase: IfPhase::FalseCase,
                    condition,
                    true_case: Some(result.clone()),
                },
                work: NormWork::Proc {
                    proc: false_proc,
                    input: ProcVisitInputs {
                        par: Par::default(),
                        bound_map_chain,
                        free_map: result.free_map,
                    },
                },
            })
        }

        IfPhase::FalseCase => {
            let target_result =
                condition.expect("combine_p_if: the condition is filled before the true case");
            let true_case_body =
                true_case.expect("combine_p_if: the true case is filled before the false case");
            let false_case_body = result;

            // ★ Leg-1: three shallow reads, then three MOVES.
            let cond_lf = target_result.par.locally_free.clone();
            let true_lf = true_case_body.par.locally_free.clone();
            let false_lf = false_case_body.par.locally_free.clone();
            let connective_used = target_result.par.connective_used
                || true_case_body.par.connective_used
                || false_case_body.par.connective_used;
            let desugared_if = If {
                condition: Some(target_result.par),
                if_true: Some(true_case_body.par),
                if_false: Some(false_case_body.par),
                locally_free: union(union(cond_lf, true_lf), false_lf),
                connective_used,
            };

            // `normalize_p_if` prepends onto its own (empty) `par`; the
            // `IfThenElse` arm then appends the caller's.
            let updated_par = Par::default().prepend_if(desugared_if);
            Ok(Step::Done(NormVal::Proc(ProcVisitOutputs {
                par: updated_par.append(outer_par),
                free_map: false_case_body.free_map,
            })))
        }
    }
}


// See rholang/src/test/scala/coop/rchain/rholang/interpreter/compiler/normalizer/ProcMatcherSpec.scala
#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use models::create_bit_vector;
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::{EEq, Expr, If, Par};
    use models::rust::utils::{
        new_boundvar_par, new_gbool_par, new_gint_expr, new_gint_par, new_new_par, new_send_par,
    };

    use crate::rust::interpreter::test_utils::utils::proc_visit_inputs_and_env;

    #[test]
    fn p_if_else_should_emit_if_node() {
        // if (true) { @Nil!(47) }
        use rholang_parser::ast::SendType;

        use crate::rust::interpreter::compiler::normalize::normalize_ann_proc;
        use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

        let (inputs, env) = proc_visit_inputs_and_env();
        let parser = rholang_parser::RholangParser::new();

        let condition = ParBuilderUtil::create_ast_bool_literal(true, &parser);
        let nil_proc = ParBuilderUtil::create_ast_nil(&parser);
        let channel = ParBuilderUtil::create_ast_quote_name(nil_proc);
        let input_47 = ParBuilderUtil::create_ast_long_literal(47, &parser);
        let if_true =
            ParBuilderUtil::create_ast_send(channel, SendType::Single, vec![input_47], &parser);
        let if_then_else =
            ParBuilderUtil::create_ast_if_then_else(condition, if_true, None, &parser);

        let result = normalize_ann_proc(&if_then_else, inputs.clone(), &env, &parser);
        assert!(result.is_ok());

        let expected_result = Par::default().prepend_if(If {
            condition: Some(new_gbool_par(true, Vec::new(), false)),
            if_true: Some(Par::default().with_sends(vec![models::rhoapi::Send {
                chan: Some(Par::default()),
                data: vec![new_gint_par(47, Vec::new(), false)],
                persistent: false,
                locally_free: Vec::new(),
                connective_used: false,
            }])),
            if_false: Some(Par::default()),
            locally_free: Vec::new(),
            connective_used: false,
        });

        assert_eq!(result.clone().unwrap().par, expected_result);
        assert_eq!(result.unwrap().free_map, inputs.free_map);
    }

    #[test]
    fn p_if_else_should_not_mix_par_from_the_input_with_normalized_one() {
        use crate::rust::interpreter::compiler::normalize::normalize_ann_proc;
        use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

        let (mut inputs, env) = proc_visit_inputs_and_env();
        inputs.par = Par::default().with_exprs(vec![new_gint_expr(7)]);

        let parser = rholang_parser::RholangParser::new();

        // if (true) { 10 }
        let condition = ParBuilderUtil::create_ast_bool_literal(true, &parser);
        let if_true = ParBuilderUtil::create_ast_long_literal(10, &parser);
        let if_then_else =
            ParBuilderUtil::create_ast_if_then_else(condition, if_true, None, &parser);

        let result = normalize_ann_proc(&if_then_else, inputs.clone(), &env, &parser);
        assert!(result.is_ok());

        let expected_if = If {
            condition: Some(new_gbool_par(true, Vec::new(), false)),
            if_true: Some(new_gint_par(10, Vec::new(), false)),
            if_false: Some(Par::default()),
            locally_free: Vec::new(),
            connective_used: false,
        };
        let expected_result = Par::default()
            .with_exprs(vec![new_gint_expr(7)])
            .prepend_if(expected_if);

        assert_eq!(result.clone().unwrap().par, expected_result);
        assert_eq!(result.unwrap().free_map, inputs.free_map);
    }

    #[test]
    fn p_if_else_should_handle_a_more_complicated_if_statement_with_an_else_clause() {
        // if (47 == 47) { new x in { x!(47) } } else { new y in { y!(47) } }
        use rholang_parser::ast::{BinaryExpOp, Id, Name, SendType, Var};
        use rholang_parser::SourcePos;

        use crate::rust::interpreter::compiler::normalize::normalize_ann_proc;
        use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

        let (inputs, env) = proc_visit_inputs_and_env();
        let parser = rholang_parser::RholangParser::new();

        // Condition: 47 == 47
        let left_47 = ParBuilderUtil::create_ast_long_literal(47, &parser);
        let right_47 = ParBuilderUtil::create_ast_long_literal(47, &parser);
        let condition =
            ParBuilderUtil::create_ast_binary_exp(BinaryExpOp::Eq, left_47, right_47, &parser);

        // If true: new x in { x!(47) }
        let x_var = Var::Id(Id {
            name: "x",
            pos: SourcePos { line: 0, col: 0 },
        });
        let x_channel = Name::NameVar(x_var);
        let x_input_47 = ParBuilderUtil::create_ast_long_literal(47, &parser);
        let x_send =
            ParBuilderUtil::create_ast_send(x_channel, SendType::Single, vec![x_input_47], &parser);
        let if_true = ParBuilderUtil::create_ast_new(vec![x_var], x_send, &parser);

        // If false: new y in { y!(47) }
        let y_var = Var::Id(Id {
            name: "y",
            pos: SourcePos { line: 0, col: 0 },
        });
        let y_channel = Name::NameVar(y_var);
        let y_input_47 = ParBuilderUtil::create_ast_long_literal(47, &parser);
        let y_send =
            ParBuilderUtil::create_ast_send(y_channel, SendType::Single, vec![y_input_47], &parser);
        let if_false = ParBuilderUtil::create_ast_new(vec![y_var], y_send, &parser);

        let if_then_else =
            ParBuilderUtil::create_ast_if_then_else(condition, if_true, Some(if_false), &parser);

        let result = normalize_ann_proc(&if_then_else, inputs.clone(), &env, &parser);
        assert!(result.is_ok());

        let new_x_branch = new_new_par(
            1,
            new_send_par(
                new_boundvar_par(0, create_bit_vector(&vec![0]), false),
                vec![new_gint_par(47, Vec::new(), false)],
                false,
                create_bit_vector(&vec![0]),
                false,
                create_bit_vector(&vec![0]),
                false,
            ),
            vec![],
            BTreeMap::new(),
            Vec::new(),
            Vec::new(),
            false,
        );
        let new_y_branch = new_new_par(
            1,
            new_send_par(
                new_boundvar_par(0, create_bit_vector(&vec![0]), false),
                vec![new_gint_par(47, Vec::new(), false)],
                false,
                create_bit_vector(&vec![0]),
                false,
                create_bit_vector(&vec![0]),
                false,
            ),
            vec![],
            BTreeMap::new(),
            Vec::new(),
            Vec::new(),
            false,
        );

        let expected_result = Par::default().prepend_if(If {
            condition: Some(Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::EEqBody(EEq {
                    p1: Some(new_gint_par(47, Vec::new(), false)),
                    p2: Some(new_gint_par(47, Vec::new(), false)),
                })),
            }])),
            if_true: Some(new_x_branch),
            if_false: Some(new_y_branch),
            locally_free: Vec::new(),
            connective_used: false,
        });

        assert_eq!(result.clone().unwrap().par, expected_result);
        assert_eq!(result.unwrap().free_map, inputs.free_map);
    }
}
