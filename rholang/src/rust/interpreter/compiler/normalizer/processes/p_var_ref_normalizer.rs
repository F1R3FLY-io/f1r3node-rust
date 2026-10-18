use std::result::Result;

use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::{Connective, VarRef};
use models::rust::par_construction::ParConstruction;
use rholang_parser::ast::{Id, VarRefKind};

use crate::rust::interpreter::compiler::exports::{
    BoundContext, ProcVisitInputs as PublicProcVisitInputs,
    ProcVisitOutputs as PublicProcVisitOutputs,
};
use crate::rust::interpreter::compiler::normalize::VarSort;
use crate::rust::interpreter::compiler::normalize_drive::{ProcVisitInputs, ProcVisitOutputs};
use crate::rust::interpreter::errors::InterpreterError;
use crate::rust::interpreter::matcher::has_locally_free::{
    connective_connective_used_ref, connective_locally_free_ref,
};
use crate::rust::interpreter::util::prepend_connective;

pub fn normalize_p_var_ref(
    var_ref_kind: VarRefKind,
    var_id: &Id,
    input: PublicProcVisitInputs,
    var_ref_span: rholang_parser::SourceSpan,
) -> Result<PublicProcVisitOutputs, InterpreterError> {
    fn success(index: i32, depth: i32, input: PublicProcVisitInputs) -> PublicProcVisitOutputs {
        let chain_depth = input.bound_map_chain.depth() as i32;
        PublicProcVisitOutputs {
            par: prepend_connective(
                input.par,
                Connective {
                    connective_instance: Some(ConnectiveInstance::VarRefBody(VarRef {
                        index,
                        depth,
                    })),
                },
                chain_depth,
            ),
            free_map: input.free_map,
        }
    }

    match input.bound_map_chain.find(var_id.name) {
        Some((context, depth)) => match (context.typ, var_ref_kind) {
            (VarSort::ProcSort, VarRefKind::Proc) => {
                Ok(success(context.index as i32, depth as i32, input))
            }
            (VarSort::NameSort, VarRefKind::Name) => {
                Ok(success(context.index as i32, depth as i32, input))
            }
            (VarSort::ProcSort, _) => Err(InterpreterError::UnexpectedProcContext {
                var_name: var_id.name.to_string(),
                name_var_source_span: context.source_span,
                process_source_span: var_ref_span,
            }),
            (VarSort::NameSort, _) => Err(InterpreterError::UnexpectedNameContext {
                var_name: var_id.name.to_string(),
                proc_var_source_span: context.source_span,
                name_source_span: var_ref_span,
            }),
        },
        None => Err(InterpreterError::UnboundVariableRefSpan {
            var_name: var_id.name.to_string(),
            source_span: var_ref_span,
        }),
    }
}

pub(crate) fn plan_p_var_ref(
    var_ref_kind: VarRefKind,
    var_id: &Id,
    input: ProcVisitInputs,
    var_ref_span: rholang_parser::SourceSpan,
    construction: &mut ParConstruction,
) -> Result<ProcVisitOutputs, InterpreterError> {
    fn success(
        index: i32,
        depth: i32,
        input: ProcVisitInputs,
        construction: &mut ParConstruction,
    ) -> Result<ProcVisitOutputs, InterpreterError> {
        let connective = Connective {
            connective_instance: Some(ConnectiveInstance::VarRefBody(VarRef { index, depth })),
        };
        let locally_free =
            connective_locally_free_ref(&connective, input.bound_map_chain.depth() as i32);
        let connective_used = connective_connective_used_ref(&connective);
        let component = construction.connective_component(&connective, &[])?;
        Ok(ProcVisitOutputs {
            par: construction.append_component(
                input.par,
                component,
                locally_free,
                connective_used,
            )?,
            free_map: input.free_map,
        })
    }

    match input.bound_map_chain.find(var_id.name) {
        Some((
            BoundContext {
                index,
                typ,
                source_span,
            },
            depth,
        )) => match typ {
            VarSort::ProcSort => match var_ref_kind {
                VarRefKind::Proc => success(index as i32, depth as i32, input, construction),

                _ => Err(InterpreterError::UnexpectedProcContext {
                    var_name: var_id.name.to_string(),
                    name_var_source_span: source_span,
                    process_source_span: var_ref_span,
                }),
            },
            VarSort::NameSort => match var_ref_kind {
                VarRefKind::Name => success(index as i32, depth as i32, input, construction),

                _ => Err(InterpreterError::UnexpectedNameContext {
                    var_name: var_id.name.to_string(),
                    proc_var_source_span: source_span,
                    name_source_span: var_ref_span,
                }),
            },
        },

        None => Err(InterpreterError::UnboundVariableRefSpan {
            var_name: var_id.name.to_string(),
            source_span: var_ref_span,
        }),
    }
}

#[cfg(test)]
mod tests {
    use models::create_bit_vector;
    use models::rhoapi::connective::ConnectiveInstance::VarRefBody;
    use models::rhoapi::{
        Connective, Match as model_match, MatchCase, Par, Receive, ReceiveBind,
        VarRef as model_VarRef,
    };
    use models::rust::utils::new_gint_par;
    use pretty_assertions::assert_eq;
    use rholang_parser::ast::{Bind, Case, Id, Names, Source, VarRefKind};
    use rholang_parser::SourcePos;

    use crate::rust::interpreter::compiler::normalize::normalize_ann_proc;
    use crate::rust::interpreter::compiler::normalize::VarSort::{NameSort, ProcSort};
    use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;
    use crate::rust::interpreter::test_utils::utils::{
        proc_visit_inputs_and_env, proc_visit_inputs_with_updated_bound_map_chain,
    };

    #[test]
    fn p_var_ref_should_do_deep_lookup_in_match_case() {
        let (inputs, env) = proc_visit_inputs_and_env();
        let bound_inputs =
            proc_visit_inputs_with_updated_bound_map_chain(inputs.clone(), "x", ProcSort);
        let parser = rholang_parser::RholangParser::new();

        // Create pattern: =x (VarRef)
        let pattern = ParBuilderUtil::create_ast_var_ref(
            VarRefKind::Proc,
            Id {
                name: "x",
                pos: SourcePos { line: 0, col: 0 },
            },
            &parser,
        );

        // Create case body: Nil
        let case_body = ParBuilderUtil::create_ast_nil(&parser);

        // Create match expression: 7
        let expression = ParBuilderUtil::create_ast_long_literal(7, &parser);

        // Create match: match 7 { case =x => Nil }
        let match_proc = ParBuilderUtil::create_ast_match(
            expression,
            vec![Case {
                pattern,
                guard: None,
                proc: case_body,
            }],
            &parser,
        );

        let result = normalize_ann_proc(&match_proc, bound_inputs.clone(), &env, &parser);
        let expected_result = bound_inputs
            .par
            .clone()
            .with_matches(vec![
                (model_match {
                    target: Some(new_gint_par(7, Vec::new(), false)),

                    cases: vec![MatchCase {
                        pattern: Some(
                            models::par_from_default! {
                                connectives: vec![Connective {
                                    connective_instance: Some(VarRefBody(model_VarRef {
                                        index: 0,
                                        depth: 1,
                                    })),
                                }],
                                ..Par::default().clone()
                            }
                            .with_locally_free(create_bit_vector(&vec![0])),
                        ),
                        source: Some(Par::default()),
                        free_count: 0,
                        guard: None,
                    }],

                    locally_free: create_bit_vector(&vec![0]),
                    connective_used: false,
                }),
            ])
            .with_locally_free(create_bit_vector(&vec![0]));

        assert_eq!(result.clone().unwrap().par, expected_result);
        assert_eq!(result.clone().unwrap().free_map, inputs.free_map);
        assert_eq!(
            result.clone().unwrap().par.locally_free,
            create_bit_vector(&vec![0])
        );
    }

    #[test]
    fn p_var_ref_should_do_deep_lookup_in_receive_case() {
        let (inputs, env) = proc_visit_inputs_and_env();
        let bound_inputs =
            proc_visit_inputs_with_updated_bound_map_chain(inputs.clone(), "x", NameSort);
        let parser = rholang_parser::RholangParser::new();

        // Create pattern: @=x (Quote of VarRef)
        let var_ref = ParBuilderUtil::create_ast_var_ref(
            VarRefKind::Name,
            Id {
                name: "x",
                pos: SourcePos { line: 0, col: 0 },
            },
            &parser,
        );
        let pattern_name = ParBuilderUtil::create_ast_quote_name(var_ref);

        // Create channel: @Nil
        let nil_proc = ParBuilderUtil::create_ast_nil(&parser);
        let channel_name = ParBuilderUtil::create_ast_quote_name(nil_proc);

        // Create bind: @=x <- @Nil
        let bind = Bind::Linear {
            lhs: Names {
                names: smallvec::SmallVec::from_vec(vec![pattern_name]),
                remainder: None,
            },
            rhs: Source::Simple { name: channel_name },
        };

        // Create continuation body: Nil
        let cont_body = ParBuilderUtil::create_ast_nil(&parser);

        // Create for comprehension: for (@=x <- @Nil) { Nil }
        let for_comprehension =
            ParBuilderUtil::create_ast_for_comprehension(vec![vec![bind]], cont_body, &parser);

        let result = normalize_ann_proc(&for_comprehension, bound_inputs.clone(), &env, &parser);
        let expected_result = inputs
            .par
            .clone()
            .with_receives(vec![Receive {
                binds: vec![ReceiveBind {
                    patterns: vec![models::par_from_default! {
                        connectives: vec![Connective {
                            connective_instance: Some(VarRefBody(model_VarRef {
                                index: 0,
                                depth: 1,
                            })),
                        }],
                        ..Par::default().clone()
                    }
                    .with_locally_free(create_bit_vector(&vec![0]))],
                    source: Some(Par::default()),
                    remainder: None,
                    free_count: 0,
                    cost_region: None,
                }],
                body: Some(Par::default()),
                persistent: false,
                peek: false,
                bind_count: 0,
                locally_free: create_bit_vector(&vec![0]),
                connective_used: false,
                condition: None,
                cost_region: None,
            }])
            .with_locally_free(create_bit_vector(&vec![0]));

        assert_eq!(result.clone().unwrap().par, expected_result);
        assert_eq!(result.clone().unwrap().free_map, inputs.free_map);
        assert_eq!(
            result.unwrap().par.locally_free,
            create_bit_vector(&vec![0])
        )
    }
}
