use models::rhoapi::{Par, Receive, ReceiveBind};
use models::rust::utils::union;
use rholang_parser::ast::{AnnProc, Name, Names};

use crate::rust::interpreter::compiler::exports::{
    FreeMap, NameVisitInputs, ProcVisitInputs, ProcVisitOutputs,
};
use crate::rust::interpreter::compiler::normalize::VarSort;
use crate::rust::interpreter::compiler::normalize_drive::{
    ContrK, NormKont, NormVal, NormWork, Step,
};
use crate::rust::interpreter::compiler::normalizer::cost_accounting::pattern_guard::reject_cost_syntax_in_name_pattern;
use crate::rust::interpreter::compiler::normalizer::processes::utils::fail_on_invalid_connective;
use crate::rust::interpreter::compiler::normalizer::remainder_normalizer_matcher::normalize_match_name;
use crate::rust::interpreter::errors::InterpreterError;
use crate::rust::interpreter::util::filter_and_adjust_bitset;

/// `contract c(x, y) = { P }`, descend half — the CONTRACT NAME first.
///
/// The child order is `name`, then every formal, then the body, and it carries
/// **two independent free maps**: the contract name extends the *enclosing*
/// free map, while the formals accumulate a *fresh* one whose bindings are then
/// absorbed into the body's `bound_map_chain`. `ContrK::idx` indexes that
/// sequence: `0` = the name, `1..=n` = formal `idx − 1`, `n + 1` = the body.
#[inline(never)]
pub(crate) fn descend_p_contr<'ast>(
    name: Name<'ast>,
    formals: &'ast Names<'ast>,
    body: AnnProc<'ast>,
    input: ProcVisitInputs,
) -> Step<'ast> {
    let child_input = NameVisitInputs {
        bound_map_chain: input.bound_map_chain.clone(),
        free_map: input.free_map.clone(),
    };
    Step::Descend {
        kont: NormKont::Contr(Box::new(ContrK {
            formals,
            idx: 0,
            name_out: None,
            acc_patterns: Vec::with_capacity(formals.names.len()),
            acc_free: FreeMap::<VarSort>::default(),
            acc_locally_free: Vec::new(),
            remainder: None,
            bound_count: 0,
            body,
            input,
        })),
        work: NormWork::Name {
            name,
            input: child_input,
        },
    }
}

/// `contract c(x, y) = { P }`, combine half.
#[inline(never)]
pub(crate) fn combine_p_contr<'ast>(
    mut k: Box<ContrK<'ast>>,
    value: NormVal,
) -> Result<Step<'ast>, InterpreterError> {
    let n_formals = k.formals.names.len();

    if k.idx == 0 {
        // The contract NAME has just finished.
        k.name_out = Some(value.into_name());
    } else if k.idx <= n_formals {
        // Formal `idx - 1` has just finished.
        let res = value.into_name();
        let result = fail_on_invalid_connective(&k.input, &res)?;

        // Accumulate the result.
        // ★ Leg-1 at the reader AND the accumulator: `HasLocallyFree<Par>` is a
        // by-value field read, so the recursive form paid TWO deep clones per
        // formal to obtain one cached bitset.
        let formal_locally_free = result.par.locally_free.clone();
        k.acc_patterns.insert(0, result.par);
        k.acc_free = result.free_map;
        k.acc_locally_free = union(std::mem::take(&mut k.acc_locally_free), formal_locally_free);
    } else {
        // The BODY has just finished.
        let body_result = value.into_proc();
        let ContrK {
            name_out,
            acc_patterns,
            acc_locally_free,
            remainder,
            bound_count,
            mut input,
            ..
        } = *k;
        let name_match_result =
            name_out.expect("combine_p_contr: the contract name is filled before the body");

        // ★ Leg-1: shallow reads, then MOVE.
        let name_locally_free = name_match_result.par.locally_free.clone();
        let name_connective_used = name_match_result.par.connective_used;
        let body_locally_free = body_result.par.locally_free.clone();
        let body_connective_used = body_result.par.connective_used;
        let receive = Receive {
            binds: vec![ReceiveBind {
                patterns: acc_patterns.into_iter().rev().collect(),
                source: Some(name_match_result.par),
                remainder,
                free_count: bound_count as i32,
            }],
            body: Some(body_result.par),
            persistent: true,
            peek: false,
            bind_count: bound_count as i32,
            locally_free: union(
                name_locally_free,
                union(
                    acc_locally_free,
                    filter_and_adjust_bitset(body_locally_free, bound_count),
                ),
            ),
            connective_used: name_connective_used || body_connective_used,
            condition: None,
        };
        //TODO: I should create new Expr for prepend_expr and provide it instead of receive.clone().into
        let updated_par = input.par.prepend_receive(receive);
        return Ok(Step::Done(NormVal::Proc(ProcVisitOutputs {
            par: updated_par,
            free_map: body_result.free_map,
        })));
    }

    // Schedule the next formal, or — once they are all in — the body.
    if k.idx < n_formals {
        // Copy the formal out of the arena-backed `Names` before `k` moves onto
        // the work stack: `Name<'ast>` is `Copy`, so the copy outlives the borrow.
        let formals: &'ast Names<'ast> = k.formals;
        // Reject cost syntax in contract-formal pattern position (W1 §1.5): a
        // signed term / token stack inside a formal `@{...}` is a process form
        // (recognized + metered), not a contract pattern. The guard walks the
        // arena, so it is handed the arena-backed reference; `Name<'ast>` is
        // `Copy`, so the value the machine descends into is copied out of it.
        reject_cost_syntax_in_name_pattern(&formals.names[k.idx])?;
        let name: Name<'ast> = formals.names[k.idx];
        let child_input = NameVisitInputs {
            bound_map_chain: k.input.bound_map_chain.push(),
            free_map: k.acc_free.clone(),
        };
        k.idx += 1;
        return Ok(Step::Descend {
            kont: NormKont::Contr(k),
            work: NormWork::Name {
                name,
                input: child_input,
            },
        });
    }

    // All formals are in: absorb the remainder and open the body's scope.
    let remainder_result = normalize_match_name(&k.formals.remainder, k.acc_free.clone())?;
    let new_enw = k
        .input
        .bound_map_chain
        .absorb_free_span(&remainder_result.1);
    k.remainder = remainder_result.0;
    k.bound_count = remainder_result.1.count_no_wildcards();

    let name_free_map = k
        .name_out
        .as_ref()
        .expect("combine_p_contr: the contract name is filled before the body")
        .free_map
        .clone();
    let body = k.body;
    k.idx += 1;
    Ok(Step::Descend {
        kont: NormKont::Contr(k),
        work: NormWork::Proc {
            proc: body,
            input: ProcVisitInputs {
                par: Par::default(),
                bound_map_chain: new_enw,
                free_map: name_free_map,
            },
        },
    })
}

// See rholang/src/test/scala/coop/rchain/rholang/interpreter/compiler/normalizer/ProcMatcherSpec.scala
#[cfg(test)]
mod tests {
    use models::create_bit_vector;
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::{EPlus, Expr, Par, Receive, ReceiveBind};
    use models::rust::utils::{new_boundvar_par, new_freevar_par, new_gint_par, new_send_par};

    use crate::rust::interpreter::compiler::normalize::VarSort;
    use crate::rust::interpreter::errors::InterpreterError;
    use crate::rust::interpreter::test_utils::utils::proc_visit_inputs_and_env;

    #[test]
    fn p_contr_should_handle_a_basic_contract() {
        use rholang_parser::ast::SendType;
        use rholang_parser::SourcePos;

        use crate::rust::interpreter::compiler::normalize::normalize_ann_proc;
        use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

        /*  new add in {
             contract add(ret, @x, @y) = {
               ret!(x + y)
             }
           }
           // new is simulated by bindings.
        */

        let (mut inputs, env) = proc_visit_inputs_and_env();
        inputs.bound_map_chain =
            inputs
                .bound_map_chain
                .put_pos(("add".to_string(), VarSort::NameSort, SourcePos {
                    line: 0,
                    col: 0,
                }));

        let parser = rholang_parser::RholangParser::new();

        // Build formals: ret, @x, @y
        let ret_name = ParBuilderUtil::create_ast_name_from_var("ret");
        let x_proc = ParBuilderUtil::create_ast_proc_var("x", &parser);
        let x_quote = ParBuilderUtil::create_ast_quote_name(x_proc);
        let y_proc = ParBuilderUtil::create_ast_proc_var("y", &parser);
        let y_quote = ParBuilderUtil::create_ast_quote_name(y_proc);
        let formals = ParBuilderUtil::create_ast_names(vec![ret_name, x_quote, y_quote], None);

        // Build body: ret!(x + y)
        let x_var = ParBuilderUtil::create_ast_proc_var("x", &parser);
        let y_var = ParBuilderUtil::create_ast_proc_var("y", &parser);
        let add_expr = ParBuilderUtil::create_ast_add(x_var, y_var, &parser);
        let ret_channel = ParBuilderUtil::create_ast_name_from_var("ret");
        let body =
            ParBuilderUtil::create_ast_send(ret_channel, SendType::Single, vec![add_expr], &parser);

        // Build contract
        let add_name = ParBuilderUtil::create_ast_name_from_var("add");
        let p_contract = ParBuilderUtil::create_ast_contract(add_name, formals, body, &parser);

        let result = normalize_ann_proc(&p_contract, inputs.clone(), &env, &parser);
        assert!(result.is_ok());

        let expected_result = inputs.par.prepend_receive(Receive {
            binds: vec![ReceiveBind {
                patterns: vec![
                    new_freevar_par(0, Vec::new()),
                    new_freevar_par(1, Vec::new()),
                    new_freevar_par(2, Vec::new()),
                ],
                source: Some(new_boundvar_par(0, create_bit_vector(&vec![0]), false)),
                remainder: None,
                free_count: 3,
            }],
            body: Some(new_send_par(
                new_boundvar_par(2, create_bit_vector(&vec![2]), false),
                vec![{
                    let mut par = Par::default().with_exprs(vec![Expr {
                        expr_instance: Some(ExprInstance::EPlusBody(EPlus {
                            p1: Some(new_boundvar_par(1, create_bit_vector(&vec![1]), false)),
                            p2: Some(new_boundvar_par(0, create_bit_vector(&vec![0]), false)),
                        })),
                    }]);
                    par.locally_free = create_bit_vector(&vec![0, 1]);
                    par
                }],
                false,
                create_bit_vector(&vec![0, 1, 2]),
                false,
                create_bit_vector(&vec![0, 1, 2]),
                false,
            )),
            persistent: true,
            peek: false,
            bind_count: 3,
            locally_free: create_bit_vector(&vec![0]),
            connective_used: false,
            condition: None,
        });

        assert_eq!(result.clone().unwrap().par, expected_result);
        assert_eq!(result.unwrap().free_map, inputs.free_map);
    }

    #[test]
    fn p_contr_should_not_count_ground_values_in_the_formals_towards_the_bind_count() {
        use rholang_parser::ast::SendType;
        use rholang_parser::SourcePos;

        use crate::rust::interpreter::compiler::normalize::normalize_ann_proc;
        use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

        /*  new ret5 in {
             contract ret5(ret, @5) = {
               ret!(5)
             }
           }
           // new is simulated by bindings.
        */

        let (mut inputs, env) = proc_visit_inputs_and_env();
        inputs.bound_map_chain =
            inputs
                .bound_map_chain
                .put_pos(("ret5".to_string(), VarSort::NameSort, SourcePos {
                    line: 0,
                    col: 0,
                }));

        let parser = rholang_parser::RholangParser::new();

        // Build formals: ret, @5
        let ret_name = ParBuilderUtil::create_ast_name_from_var("ret");
        let five_proc = ParBuilderUtil::create_ast_int(5, &parser);
        let five_quote = ParBuilderUtil::create_ast_quote_name(five_proc);
        let formals = ParBuilderUtil::create_ast_names(vec![ret_name, five_quote], None);

        // Build body: ret!(5)
        let five_literal = ParBuilderUtil::create_ast_int(5, &parser);
        let ret_channel = ParBuilderUtil::create_ast_name_from_var("ret");
        let body = ParBuilderUtil::create_ast_send(
            ret_channel,
            SendType::Single,
            vec![five_literal],
            &parser,
        );

        // Build contract
        let ret5_name = ParBuilderUtil::create_ast_name_from_var("ret5");
        let p_contract = ParBuilderUtil::create_ast_contract(ret5_name, formals, body, &parser);

        let result = normalize_ann_proc(&p_contract, inputs.clone(), &env, &parser);
        assert!(result.is_ok());

        let expected_result = inputs.par.prepend_receive(Receive {
            binds: vec![ReceiveBind {
                patterns: vec![
                    new_freevar_par(0, Vec::new()),
                    new_gint_par(5, Vec::new(), false),
                ],
                source: Some(new_boundvar_par(0, create_bit_vector(&vec![0]), false)),
                remainder: None,
                free_count: 1,
            }],
            body: Some(new_send_par(
                new_boundvar_par(0, create_bit_vector(&vec![0]), false),
                vec![new_gint_par(5, Vec::new(), false)],
                false,
                create_bit_vector(&vec![0]),
                false,
                create_bit_vector(&vec![0]),
                false,
            )),
            persistent: true,
            peek: false,
            bind_count: 1,
            locally_free: create_bit_vector(&vec![0]),
            connective_used: false,
            condition: None,
        });

        assert_eq!(result.clone().unwrap().par, expected_result);
        assert_eq!(result.unwrap().free_map, inputs.free_map);
    }

    #[test]
    fn p_contr_should_not_compile_when_logical_or_or_not_is_used_in_the_pattern_of_the_receive() {
        use crate::rust::interpreter::compiler::compiler::Compiler;

        // Test disjunction in contract pattern
        let result1 =
            Compiler::source_to_adt(r#"new x in { contract x(@{ y /\ {Nil \/ Nil}}) = { Nil } }"#);
        assert!(result1.is_err());
        match result1 {
            Err(InterpreterError::PatternReceiveError(msg)) => {
                assert!(msg.contains("\\/ (disjunction)"));
            }
            other => panic!("Expected PatternReceiveError, got: {:?}", other),
        }

        // Test negation in contract pattern
        let result2 =
            Compiler::source_to_adt(r#"new x in { contract x(@{ y /\ ~Nil}) = { Nil } }"#);
        assert!(result2.is_err());
        match result2 {
            Err(InterpreterError::PatternReceiveError(msg)) => {
                assert!(msg.contains("~ (negation)"));
            }
            other => panic!("Expected PatternReceiveError, got: {:?}", other),
        }
    }

    #[test]
    fn p_contr_should_compile_when_logical_and_is_used_in_the_pattern_of_the_receive() {
        use crate::rust::interpreter::compiler::compiler::Compiler;

        let result1 =
            Compiler::source_to_adt(r#"new x in { contract x(@{ y /\ {Nil /\ Nil}}) = { Nil } }"#);
        assert!(
            result1.is_ok(),
            "Conjunction in contract pattern should be allowed, but got error: {:?}",
            result1
        );
    }
}
