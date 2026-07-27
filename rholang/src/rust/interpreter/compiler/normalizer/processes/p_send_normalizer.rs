use models::rhoapi::{Par, Send};
use models::rust::utils::union;
use rholang_parser::ast::{Name, SendType};

use crate::rust::interpreter::compiler::exports::{
    NameVisitInputs, ProcVisitInputs, ProcVisitOutputs,
};
use crate::rust::interpreter::compiler::normalize_drive::{
    NormKont, NormVal, NormWork, SendK, Step,
};
use crate::rust::interpreter::errors::InterpreterError;

/// `x!(P, Q, …)`, descend half — the CHANNEL first.
///
/// The channel's free map seeds the message sequence, and each message's free
/// map seeds the next, so all `1 + |inputs|` children are strictly ordered.
/// `idx == 0` in [`SendK`] means "the channel is still outstanding".
#[inline(never)]
pub(crate) fn descend_p_send<'ast>(
    channel: Name<'ast>,
    send_type: &SendType,
    inputs: &'ast rholang_parser::ast::ProcList<'ast>,
    input: ProcVisitInputs,
) -> Step<'ast> {
    let persistent = match send_type {
        rholang_parser::ast::SendType::Single => false,
        rholang_parser::ast::SendType::Multiple => true,
    };
    let input_depth = input.bound_map_chain.depth() as i32;
    Step::Descend {
        kont: NormKont::Send(Box::new(SendK {
            inputs,
            idx: 0,
            name_par: None,
            acc_pars: Vec::with_capacity(inputs.len()),
            acc_locally_free: Vec::new(),
            acc_connective_used: false,
            free_map: input.free_map.clone(),
            bound_map_chain: input.bound_map_chain.clone(),
            input_par: input.par,
            input_depth,
            persistent,
        })),
        work: NormWork::Name {
            name: channel,
            input: NameVisitInputs {
                bound_map_chain: input.bound_map_chain,
                free_map: input.free_map,
            },
        },
    }
}

/// `x!(P, Q, …)`, combine half.
#[inline(never)]
pub(crate) fn combine_p_send<'ast>(
    mut k: Box<SendK<'ast>>,
    value: NormVal,
) -> Result<Step<'ast>, InterpreterError> {
    if k.idx == 0 {
        // The CHANNEL has just finished.
        let name_match_result = value.into_name();
        k.free_map = name_match_result.free_map;
        k.name_par = Some(name_match_result.par);
    } else {
        // Message `idx - 1` has just finished.
        // ★ Leg-1: read the shallow fields, then MOVE. See
        // `collection_normalize_matcher::combine_collect`.
        let proc_match_result = value.into_proc();
        let child_locally_free = proc_match_result.par.locally_free.clone();
        let child_connective_used = proc_match_result.par.connective_used;
        k.acc_pars.push(proc_match_result.par);
        k.free_map = proc_match_result.free_map;
        k.acc_locally_free = union(std::mem::take(&mut k.acc_locally_free), child_locally_free);
        k.acc_connective_used = k.acc_connective_used || child_connective_used;
    }

    if let Some(next) = k.inputs.get(k.idx).copied() {
        let child_input = ProcVisitInputs {
            par: Par::default(),
            bound_map_chain: k.bound_map_chain.clone(),
            free_map: k.free_map.clone(),
        };
        k.idx += 1;
        return Ok(Step::Descend {
            kont: NormKont::Send(k),
            work: NormWork::Proc {
                proc: next,
                input: child_input,
            },
        });
    }

    let SendK {
        idx: _,
        name_par,
        acc_pars,
        acc_locally_free,
        acc_connective_used,
        free_map,
        bound_map_chain,
        mut input_par,
        // ⚠ Retained on the frame but INERT here: the depth argument of
        // `<Par as HasLocallyFree<Par>>::locally_free` is `_depth` — the impl
        // returns the cached field and never consults it — so removing the
        // by-value reader removed the only consumer.
        input_depth: _,
        persistent,
        ..
    } = *k;
    let name_par = name_par.expect("combine_p_send: the channel slot is filled before any message");
    let _ = bound_map_chain;

    // ★ Leg-1 at the reader. `<Par as HasLocallyFree<Par>>::locally_free` is
    // `|_, p, _| p.locally_free` and `connective_used` is `|_, p| p.connective_used`
    // — neither recurses, both take the subject BY VALUE, so the by-value
    // signature forced `name_par.clone().locally_free(name_par.clone(), d)`:
    // two full Θ(depth) deep clones to read one cached bitset. The fields are
    // read directly instead, which is byte-identical by construction.
    let name_locally_free = name_par.locally_free.clone();
    let name_connective_used = name_par.connective_used;
    let send = Send {
        chan: Some(name_par),
        data: acc_pars,
        persistent,
        locally_free: union(name_locally_free, acc_locally_free),
        connective_used: name_connective_used || acc_connective_used,
    };

    Ok(Step::Done(NormVal::Proc(ProcVisitOutputs {
        par: input_par.prepend_send(send),
        free_map,
    })))
}


// See rholang/src/test/scala/coop/rchain/rholang/interpreter/compiler/normalizer/ProcMatcherSpec.scala
#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use models::create_bit_vector;
    use models::rhoapi::Par;
    use models::rust::utils::{new_boundvar_par, new_gint_par, new_send};

    use crate::rust::interpreter::compiler::compiler::Compiler;
    use crate::rust::interpreter::compiler::exports::ProcVisitInputs;
    use crate::rust::interpreter::compiler::normalize::VarSort;
    use crate::rust::interpreter::errors::InterpreterError;
    use crate::rust::interpreter::test_utils::utils::proc_visit_inputs_and_env;

    #[test]
    fn p_send_should_handle_a_basic_send() {
        use rholang_parser::ast::SendType;

        use crate::rust::interpreter::compiler::normalize::normalize_ann_proc;
        use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

        let (mut inputs, env) = proc_visit_inputs_and_env();
        let parser = rholang_parser::RholangParser::new();

        // Create channel: @Nil
        let nil_proc = ParBuilderUtil::create_ast_nil(&parser);
        let channel = ParBuilderUtil::create_ast_quote_name(nil_proc);

        // Create inputs: 7, 8
        let input1 = ParBuilderUtil::create_ast_long_literal(7, &parser);
        let input2 = ParBuilderUtil::create_ast_long_literal(8, &parser);

        // Create send: @Nil!(7, 8)
        let send_proc = ParBuilderUtil::create_ast_send(
            channel,
            SendType::Single,
            vec![input1, input2],
            &parser,
        );

        let result = normalize_ann_proc(&send_proc, inputs.clone(), &env, &parser);
        assert!(result.is_ok());
        assert_eq!(
            result.clone().unwrap().par,
            inputs.par.prepend_send(new_send(
                Par::default(),
                vec![
                    new_gint_par(7, Vec::new(), false),
                    new_gint_par(8, Vec::new(), false)
                ],
                false,
                Vec::new(),
                false
            ))
        );
        assert_eq!(result.unwrap().free_map, inputs.free_map);
    }

    #[test]
    fn p_send_should_handle_a_name_var() {
        use rholang_parser::ast::SendType;
        use rholang_parser::SourcePos;

        use crate::rust::interpreter::compiler::normalize::normalize_ann_proc;
        use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

        let (mut inputs, env) = proc_visit_inputs_and_env();
        inputs.bound_map_chain =
            inputs
                .bound_map_chain
                .put_pos(("x".to_string(), VarSort::NameSort, SourcePos {
                    line: 0,
                    col: 0,
                }));
        let parser = rholang_parser::RholangParser::new();

        // Create channel: x (NameVar)
        let channel = ParBuilderUtil::create_ast_name_var("x");

        // Create inputs: 7, 8
        let input1 = ParBuilderUtil::create_ast_long_literal(7, &parser);
        let input2 = ParBuilderUtil::create_ast_long_literal(8, &parser);

        // Create send: x!(7, 8)
        let send_proc = ParBuilderUtil::create_ast_send(
            channel,
            SendType::Single,
            vec![input1, input2],
            &parser,
        );

        let result = normalize_ann_proc(&send_proc, inputs.clone(), &env, &parser);
        assert!(result.is_ok());
        assert_eq!(
            result.clone().unwrap().par,
            inputs.par.prepend_send(new_send(
                new_boundvar_par(0, create_bit_vector(&vec![0]), false),
                vec![
                    new_gint_par(7, Vec::new(), false),
                    new_gint_par(8, Vec::new(), false)
                ],
                false,
                create_bit_vector(&vec![0]),
                false
            ))
        );
        assert_eq!(result.unwrap().free_map, inputs.free_map);
    }

    #[test]
    fn p_send_should_propagate_known_free() {
        use rholang_parser::ast::{Id, SendType, Var};
        use rholang_parser::SourcePos;

        use crate::rust::interpreter::compiler::normalize::normalize_ann_proc;
        use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

        let parser = rholang_parser::RholangParser::new();

        // Create channel: @*x (Quote of Eval of NameVar)
        let name_var = ParBuilderUtil::create_ast_name_var("x");
        let eval_proc = ParBuilderUtil::create_ast_eval(name_var, &parser);
        let channel = ParBuilderUtil::create_ast_quote_name(eval_proc);

        // Create inputs: 7, x (ProcVar)
        let input1 = ParBuilderUtil::create_ast_long_literal(7, &parser);
        let input2 = ParBuilderUtil::create_ast_proc_var_from_var(
            Var::Id(Id {
                name: "x",
                pos: SourcePos { line: 0, col: 0 },
            }),
            &parser,
        );

        // Create send: @*x!(7, x)
        let send_proc = ParBuilderUtil::create_ast_send(
            channel,
            SendType::Single,
            vec![input1, input2],
            &parser,
        );

        let result =
            normalize_ann_proc(&send_proc, ProcVisitInputs::new(), &HashMap::new(), &parser);
        assert!(result.is_err());
        assert!(matches!(
            result,
            Err(InterpreterError::UnexpectedReuseOfProcContextFree {
                var_name,
                first_use: _,
                second_use: _
            }) if var_name == "x"
        ));
    }

    #[test]
    fn p_send_should_not_compile_if_data_contains_negation() {
        let result = Compiler::source_to_adt(r#"new x in { x!(~1) }"#);
        assert!(result.is_err());
        match result {
            Err(InterpreterError::TopLevelLogicalConnectivesNotAllowedError(msg)) => {
                assert!(msg.contains("~ (negation)"));
            }
            other => panic!(
                "Expected TopLevelLogicalConnectivesNotAllowedError, got: {:?}",
                other
            ),
        }
    }

    #[test]
    fn p_send_should_not_compile_if_data_contains_conjuction() {
        let result = Compiler::source_to_adt(r#"new x in { x!(1 /\ 2) }"#);
        assert!(result.is_err());
        match result {
            Err(InterpreterError::TopLevelLogicalConnectivesNotAllowedError(msg)) => {
                assert!(msg.contains("/\\ (conjunction)"));
            }
            other => panic!(
                "Expected TopLevelLogicalConnectivesNotAllowedError, got: {:?}",
                other
            ),
        }
    }

    #[test]
    fn p_send_should_not_compile_if_data_contains_disjunction() {
        let result = Compiler::source_to_adt(r#"new x in { x!(1 \/ 2) }"#);
        assert!(result.is_err());
        match result {
            Err(InterpreterError::TopLevelLogicalConnectivesNotAllowedError(msg)) => {
                assert!(msg.contains("\\/ (disjunction)"));
            }
            other => panic!(
                "Expected TopLevelLogicalConnectivesNotAllowedError, got: {:?}",
                other
            ),
        }
    }

    #[test]
    fn p_send_should_not_compile_if_data_contains_wildcard() {
        let result = Compiler::source_to_adt(r#"@"x"!(_)"#);
        assert!(result.is_err());
        match result {
            Err(InterpreterError::TopLevelWildcardsNotAllowedError(msg)) => {
                assert!(msg.contains("_ (wildcard)"));
            }
            other => panic!(
                "Expected TopLevelWildcardsNotAllowedError, got: {:?}",
                other
            ),
        }
    }

    #[test]
    fn p_send_should_not_compile_if_data_contains_free_variable() {
        let result = Compiler::source_to_adt(r#"@"x"!(y)"#);
        assert!(result.is_err());
        match result {
            Err(InterpreterError::TopLevelFreeVariablesNotAllowedError(msg)) => {
                assert!(msg.contains("y"));
            }
            other => panic!(
                "Expected TopLevelFreeVariablesNotAllowedError, got: {:?}",
                other
            ),
        }
    }

    #[test]
    fn p_send_should_not_compile_if_name_contains_connectives() {
        // Test conjunction in channel name
        let result1 = Compiler::source_to_adt(r#"@{Nil /\ Nil}!(1)"#);
        assert!(result1.is_err());
        match result1 {
            Err(InterpreterError::TopLevelLogicalConnectivesNotAllowedError(msg)) => {
                assert!(msg.contains("/\\ (conjunction)"));
            }
            other => panic!(
                "Expected TopLevelLogicalConnectivesNotAllowedError, got: {:?}",
                other
            ),
        }

        // Test disjunction in channel name
        let result2 = Compiler::source_to_adt(r#"@{Nil \/ Nil}!(1)"#);
        assert!(result2.is_err());
        match result2 {
            Err(InterpreterError::TopLevelLogicalConnectivesNotAllowedError(msg)) => {
                assert!(msg.contains("\\/ (disjunction)"));
            }
            other => panic!(
                "Expected TopLevelLogicalConnectivesNotAllowedError, got: {:?}",
                other
            ),
        }

        // Test negation in channel name
        let result3 = Compiler::source_to_adt(r#"@{~Nil}!(1)"#);
        assert!(result3.is_err());
        match result3 {
            Err(InterpreterError::TopLevelLogicalConnectivesNotAllowedError(msg)) => {
                assert!(msg.contains("~ (negation)"));
            }
            other => panic!(
                "Expected TopLevelLogicalConnectivesNotAllowedError, got: {:?}",
                other
            ),
        }
    }
}
