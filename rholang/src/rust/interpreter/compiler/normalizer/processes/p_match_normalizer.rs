use std::collections::HashMap;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EMatchExpr, Expr, Match, MatchCase, Par};
use models::rust::utils::union;
use rholang_parser::ast::{AnnProc, Case};

use crate::rust::interpreter::compiler::exports::{FreeMap, ProcVisitInputs, ProcVisitOutputs};
use crate::rust::interpreter::compiler::normalize::normalize_ann_proc;
use crate::rust::interpreter::errors::InterpreterError;
use crate::rust::interpreter::util::filter_and_adjust_bitset;

pub fn normalize_p_match<'ast>(
    expression: &'ast AnnProc<'ast>,
    cases: &'ast [Case<'ast>],
    input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast rholang_parser::RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    let target_result = normalize_ann_proc(
        expression,
        ProcVisitInputs {
            par: Par::default(),
            ..input.clone()
        },
        env,
        parser,
    )?;

    let mut init_acc = (vec![], target_result.free_map.clone(), Vec::new(), false);

    for case in cases {
        let Case {
            pattern,
            guard,
            proc: case_body,
        } = case;

        let pattern_result = normalize_ann_proc(
            pattern,
            ProcVisitInputs {
                par: Par::default(),
                bound_map_chain: input.bound_map_chain.push(),
                free_map: FreeMap::default(),
            },
            env,
            parser,
        )?;

        let case_env = input
            .bound_map_chain
            .absorb_free_span(&pattern_result.free_map);
        let bound_count = pattern_result.free_map.count_no_wildcards();

        // Optional `where` guard: normalized in the same scope as the
        // case body (pattern bindings absorbed into bound_map_chain).
        // No syntactic check on the guard's content (see plan §3.8) —
        // bool-ness is enforced at runtime by the matcher in Phase 6.
        let guard_result = match guard {
            Some(g) => Some(normalize_ann_proc(
                g,
                ProcVisitInputs {
                    par: Par::default(),
                    bound_map_chain: case_env.clone(),
                    free_map: init_acc.1.clone(),
                },
                env,
                parser,
            )?),
            None => None,
        };

        let case_body_result = normalize_ann_proc(
            case_body,
            ProcVisitInputs {
                par: Par::default(),
                bound_map_chain: case_env.clone(),
                free_map: guard_result
                    .as_ref()
                    .map(|gr| gr.free_map.clone())
                    .unwrap_or_else(|| init_acc.1.clone()),
            },
            env,
            parser,
        )?;

        init_acc.0.insert(0, MatchCase {
            pattern: Some(pattern_result.par.clone()),
            source: Some(case_body_result.par.clone()),
            free_count: bound_count as i32,
            guard: guard_result.as_ref().map(|gr| gr.par.clone()),
        });
        init_acc.1 = case_body_result.free_map;
        init_acc.2 = union(
            union(init_acc.2.clone(), pattern_result.par.locally_free.clone()),
            filter_and_adjust_bitset(
                {
                    let mut lf = case_body_result.par.locally_free.clone();
                    if let Some(gr) = &guard_result {
                        lf = union(lf, gr.par.locally_free.clone());
                    }
                    lf
                },
                bound_count,
            ),
        );
        init_acc.3 = init_acc.3
            || case_body_result.par.connective_used
            || guard_result
                .as_ref()
                .map(|gr| gr.par.connective_used)
                .unwrap_or(false);
    }

    let cases: Vec<MatchCase> = init_acc.0.into_iter().rev().collect();
    let locally_free = union(init_acc.2, target_result.par.locally_free.clone());
    let connective_used = init_acc.3 || target_result.par.connective_used;

    // Classification: if every case body is a single pure boolean
    // expression (and likewise for any guards), emit an `EMatchExpr` so
    // the result composes in expression contexts (e.g. `if (match x { ...
    // })`, `where match x { ... }`). Otherwise emit a process-level
    // `Match`. See plan §3.4. Safe under both contexts: an EMatchExpr
    // run as a process degrades to "evaluate the match and discard the
    // result," which matches the original Match's behaviour for pure
    // bool bodies anyway.
    if all_cases_pure_bool(&cases) {
        let ematch = EMatchExpr {
            target: Some(target_result.par.clone()),
            cases,
            locally_free,
            connective_used,
        };
        Ok(ProcVisitOutputs {
            par: input.par.clone().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::EMatchExprBody(ematch)),
            }]),
            free_map: init_acc.1,
        })
    } else {
        let result_match = Match {
            target: Some(target_result.par.clone()),
            cases,
            locally_free,
            connective_used,
        };
        Ok(ProcVisitOutputs {
            par: input.par.clone().prepend_match(result_match.clone()),
            free_map: init_acc.1,
        })
    }
}

/// True iff every match case's body is a syntactically-recognizable bool
/// expression — exactly one Expr in the body Par, of a kind that yields
/// bool. Used to decide between emitting `Match` (process) and
/// `EMatchExpr` (expression). Strictly syntactic; doesn't try to peek
/// into bound EVars.
fn all_cases_pure_bool(cases: &[MatchCase]) -> bool {
    cases.iter().all(|c| match c.source.as_ref() {
        Some(par) => par_is_pure_bool_expr(par),
        None => false,
    })
}

fn par_is_pure_bool_expr(par: &Par) -> bool {
    if !par.sends.is_empty()
        || !par.receives.is_empty()
        || !par.news.is_empty()
        || !par.matches.is_empty()
        || !par.bundles.is_empty()
        || !par.unforgeables.is_empty()
        || !par.connectives.is_empty()
        || !par.conditionals.is_empty()
        || par.exprs.len() != 1
    {
        return false;
    }
    matches!(
        par.exprs[0].expr_instance,
        Some(ExprInstance::GBool(_))
            | Some(ExprInstance::ENotBody(_))
            | Some(ExprInstance::EAndBody(_))
            | Some(ExprInstance::EOrBody(_))
            | Some(ExprInstance::EEqBody(_))
            | Some(ExprInstance::ENeqBody(_))
            | Some(ExprInstance::ELtBody(_))
            | Some(ExprInstance::ELteBody(_))
            | Some(ExprInstance::EGtBody(_))
            | Some(ExprInstance::EGteBody(_))
            | Some(ExprInstance::EMatchesBody(_))
            | Some(ExprInstance::EMatchExprBody(_))
    )
}

// See rholang/src/test/scala/coop/rchain/rholang/interpreter/compiler/normalizer/ProcMatcherSpec.scala
#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use models::create_bit_vector;
    use models::rhoapi::{Match, MatchCase, Par, Receive, ReceiveBind};
    use models::rust::utils::{
        new_boundvar_par, new_elist_par, new_freevar_expr, new_freevar_par, new_gint_par, new_send,
        new_wildcard_par,
    };

    use crate::rust::interpreter::compiler::exports::ProcVisitInputs;
    use crate::rust::interpreter::compiler::normalize::VarSort;
    use crate::rust::interpreter::errors::InterpreterError;
    use crate::rust::interpreter::test_utils::utils::proc_visit_inputs_and_env;
    use crate::rust::interpreter::util::prepend_expr;

    #[test]
    fn p_match_should_fail_if_a_free_variable_is_used_twice_in_the_target() {
        // match 47 { case (y | y) => Nil }
        use rholang_parser::ast::Case;

        use crate::rust::interpreter::compiler::normalize::normalize_ann_proc;
        use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

        let parser = rholang_parser::RholangParser::new();

        // Create expression: 47
        let expression = ParBuilderUtil::create_ast_long_literal(47, &parser);

        // Create pattern: y | y (Par of two evals)
        let y_eval_left =
            ParBuilderUtil::create_ast_eval(ParBuilderUtil::create_ast_name_var("y"), &parser);
        let y_eval_right =
            ParBuilderUtil::create_ast_eval(ParBuilderUtil::create_ast_name_var("y"), &parser);
        let pattern = ParBuilderUtil::create_ast_par(y_eval_left, y_eval_right, &parser);

        // Create case proc: Nil
        let case_proc = ParBuilderUtil::create_ast_nil(&parser);

        // Create match
        let p_match = ParBuilderUtil::create_ast_match(
            expression,
            vec![Case {
                pattern,
                guard: None,
                proc: case_proc,
            }],
            &parser,
        );

        let result = normalize_ann_proc(
            &p_match,
            proc_visit_inputs_and_env().0,
            &proc_visit_inputs_and_env().1,
            &parser,
        );
        assert!(result.is_err());
        assert!(matches!(
            result,
            Err(InterpreterError::UnexpectedReuseOfNameContextFree {
                var_name,
                first_use: _,
                second_use: _
            }) if var_name == "y"
        ));
    }

    #[test]
    fn p_match_should_have_a_free_count_of_1_if_the_case_contains_a_wildcard_and_a_free_variable() {
        use rholang_parser::ast::{Case, Var};
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

        // Create match x { case [y, _] => Nil ; case _ => Nil } using new AST
        // Expression: *x (eval of x)
        let expression =
            ParBuilderUtil::create_ast_eval(ParBuilderUtil::create_ast_name_var("x"), &parser);

        // First case pattern: [y, _]
        let y_eval =
            ParBuilderUtil::create_ast_eval(ParBuilderUtil::create_ast_name_var("y"), &parser);
        let wildcard_eval = ParBuilderUtil::create_ast_eval(
            rholang_parser::ast::Name::NameVar(Var::Wildcard),
            &parser,
        );
        let list_pattern =
            ParBuilderUtil::create_ast_list(vec![y_eval, wildcard_eval], None, &parser);
        let nil_proc1 = ParBuilderUtil::create_ast_nil(&parser);

        // Second case pattern: _
        let wildcard_pattern = ParBuilderUtil::create_ast_eval(
            rholang_parser::ast::Name::NameVar(Var::Wildcard),
            &parser,
        );
        let nil_proc2 = ParBuilderUtil::create_ast_nil(&parser);

        let p_match = ParBuilderUtil::create_ast_match(
            expression,
            vec![
                Case {
                    pattern: list_pattern,
                    guard: None,
                    proc: nil_proc1,
                },
                Case {
                    pattern: wildcard_pattern,
                    guard: None,
                    proc: nil_proc2,
                },
            ],
            &parser,
        );

        let result = normalize_ann_proc(&p_match, inputs, &env, &parser);
        assert!(result.is_ok());

        let expected_result = Par::default().prepend_match(Match {
            target: Some(new_boundvar_par(0, create_bit_vector(&vec![0]), false)),
            cases: vec![
                MatchCase {
                    pattern: Some(new_elist_par(
                        vec![
                            new_freevar_par(0, Vec::new()),
                            new_wildcard_par(Vec::new(), true),
                        ],
                        Vec::new(),
                        true,
                        None,
                        Vec::new(),
                        true,
                    )),
                    source: Some(Par::default()),
                    free_count: 1,
                    guard: None,
                },
                MatchCase {
                    pattern: Some(new_wildcard_par(Vec::new(), true)),
                    source: Some(Par::default()),
                    free_count: 0,
                    guard: None,
                },
            ],
            locally_free: create_bit_vector(&vec![0]),
            connective_used: false,
        });

        assert_eq!(result.clone().unwrap().par, expected_result);
        assert_eq!(result.unwrap().par.matches[0].cases[0].free_count, 1);
    }

    #[test]
    fn p_match_should_handle_a_match_inside_a_for_comprehension() {
        // for (@x <- @Nil) { match x { case 42 => Nil ; case y => Nil } | @Nil!(47)
        use rholang_parser::ast::{Bind, Case, Names, SendType, Source};

        use crate::rust::interpreter::compiler::normalize::normalize_ann_proc;
        use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

        let parser = rholang_parser::RholangParser::new();

        // Create the complete Par structure: for (@x <- @Nil) { match x { case 42 => Nil ; case y => Nil } } | @Nil!(47)

        // Create Match body: match x { case 42 => Nil ; case y => Nil }
        let x_eval =
            ParBuilderUtil::create_ast_eval(ParBuilderUtil::create_ast_name_var("x"), &parser);
        let pattern1 = ParBuilderUtil::create_ast_long_literal(42, &parser);
        let proc1 = ParBuilderUtil::create_ast_nil(&parser);
        let pattern2 =
            ParBuilderUtil::create_ast_eval(ParBuilderUtil::create_ast_name_var("y"), &parser);
        let proc2 = ParBuilderUtil::create_ast_nil(&parser);
        let match_proc = ParBuilderUtil::create_ast_match(
            x_eval,
            vec![
                Case {
                    pattern: pattern1,
                    guard: None,
                    proc: proc1,
                },
                Case {
                    pattern: pattern2,
                    guard: None,
                    proc: proc2,
                },
            ],
            &parser,
        );

        // Create Bind: @x <- @Nil
        let x_name_eval =
            ParBuilderUtil::create_ast_eval(ParBuilderUtil::create_ast_name_var("x"), &parser);
        let x_pattern = ParBuilderUtil::create_ast_quote_name(x_name_eval);
        let nil_chan = ParBuilderUtil::create_ast_nil(&parser);
        let nil_source = ParBuilderUtil::create_ast_quote_name(nil_chan);

        let bind = Bind::Linear {
            lhs: Names {
                names: smallvec::SmallVec::from_vec(vec![x_pattern]),
                remainder: None,
            },
            rhs: Source::Simple { name: nil_source },
        };

        // Create ForComprehension
        let for_comp =
            ParBuilderUtil::create_ast_for_comprehension(vec![vec![bind]], match_proc, &parser);

        // Create Send: @Nil!(47)
        let nil_chan2 = ParBuilderUtil::create_ast_nil(&parser);
        let send_channel = ParBuilderUtil::create_ast_quote_name(nil_chan2);
        let send_input = ParBuilderUtil::create_ast_long_literal(47, &parser);
        let send_proc = ParBuilderUtil::create_ast_send(
            send_channel,
            SendType::Single,
            vec![send_input],
            &parser,
        );

        // Create Par
        let p_par = ParBuilderUtil::create_ast_par(for_comp, send_proc, &parser);

        let result = normalize_ann_proc(&p_par, ProcVisitInputs::new(), &HashMap::new(), &parser);
        assert!(result.is_ok());

        let expected_result = Par::default()
            .prepend_send(new_send(
                Par::default(),
                vec![new_gint_par(47, Vec::new(), false)],
                false,
                Vec::new(),
                false,
            ))
            .prepend_receive(Receive {
                binds: vec![ReceiveBind {
                    patterns: vec![new_freevar_par(0, Vec::new())],
                    source: Some(Par::default()),
                    remainder: None,
                    free_count: 1,
                }],
                body: Some(Par::default().prepend_match(Match {
                    target: Some(new_boundvar_par(0, create_bit_vector(&vec![0]), false)),
                    cases: vec![
                        MatchCase {
                            pattern: Some(new_gint_par(42, Vec::new(), false)),
                            source: Some(Par::default()),
                            free_count: 0,
                            guard: None,
                        },
                        MatchCase {
                            pattern: Some(new_freevar_par(0, Vec::new())),
                            source: Some(Par::default()),
                            free_count: 1,
                            guard: None,
                        },
                    ],
                    locally_free: create_bit_vector(&vec![0]),
                    connective_used: false,
                })),
                persistent: false,
                peek: false,
                bind_count: 1,
                locally_free: Vec::new(),
                connective_used: false,
                condition: None,
            });

        assert_eq!(result.clone().unwrap().par, expected_result);
        assert_eq!(result.unwrap().free_map, ProcVisitInputs::new().free_map);
    }

    #[test]
    fn p_match_should_handle_a_match_inside_a_for_pattern() {
        // for (@{match {x | y} { 47 => Nil }} <- @Nil) { Nil }
        use rholang_parser::ast::{Bind, Case, Id, Names, Source, Var};
        use rholang_parser::SourcePos;

        use crate::rust::interpreter::compiler::normalize::normalize_ann_proc;
        use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

        let parser = rholang_parser::RholangParser::new();

        // Create for (@{match {x | y} { 47 => Nil }} <- @Nil) { Nil } using new AST
        // Create match expression: x | y (Par of two ProcVars)
        let x_proc_var = ParBuilderUtil::create_ast_proc_var_from_var(
            Var::Id(Id {
                name: "x",
                pos: SourcePos { line: 0, col: 0 },
            }),
            &parser,
        );
        let y_proc_var = ParBuilderUtil::create_ast_proc_var_from_var(
            Var::Id(Id {
                name: "y",
                pos: SourcePos { line: 0, col: 0 },
            }),
            &parser,
        );
        let par_expr = ParBuilderUtil::create_ast_par(x_proc_var, y_proc_var, &parser);

        // Create match: match {x | y} { 47 => Nil }
        let pattern = ParBuilderUtil::create_ast_long_literal(47, &parser);
        let proc = ParBuilderUtil::create_ast_nil(&parser);
        let match_proc = ParBuilderUtil::create_ast_match(
            par_expr,
            vec![Case {
                pattern,
                guard: None,
                proc,
            }],
            &parser,
        );

        // Create bind: @{match} <- @Nil
        let match_pattern = ParBuilderUtil::create_ast_quote_name(match_proc);
        let nil_chan = ParBuilderUtil::create_ast_nil(&parser);
        let nil_source = ParBuilderUtil::create_ast_quote_name(nil_chan);

        let bind = Bind::Linear {
            lhs: Names {
                names: smallvec::SmallVec::from_vec(vec![match_pattern]),
                remainder: None,
            },
            rhs: Source::Simple { name: nil_source },
        };

        // Create for-comprehension body: Nil
        let body = ParBuilderUtil::create_ast_nil(&parser);

        // Create ForComprehension
        let input = ParBuilderUtil::create_ast_for_comprehension(vec![vec![bind]], body, &parser);

        let (inputs, env) = proc_visit_inputs_and_env();
        let result = normalize_ann_proc(&input, inputs.clone(), &env, &parser);
        assert!(result.is_ok());

        let expected_result = Par::default().prepend_receive(Receive {
            binds: vec![ReceiveBind {
                patterns: vec![{
                    let mut par = Par::default().with_matches(vec![Match {
                        target: Some(prepend_expr(
                            new_freevar_par(1, Vec::new()),
                            new_freevar_expr(0),
                            0,
                        )),
                        cases: vec![MatchCase {
                            pattern: Some(new_gint_par(47, Vec::new(), false)),
                            source: Some(Par::default()),
                            free_count: 0,
                            guard: None,
                        }],
                        locally_free: Vec::new(),
                        connective_used: true,
                    }]);
                    par.connective_used = true;
                    par
                }],
                source: Some(Par::default()),
                remainder: None,
                free_count: 2,
            }],
            body: Some(Par::default()),
            persistent: false,
            peek: false,
            bind_count: 2,
            locally_free: Vec::new(),
            connective_used: false,
            condition: None,
        });

        assert_eq!(result.clone().unwrap().par, expected_result);
        assert_eq!(result.unwrap().free_map, inputs.free_map);
    }

    // Phase 5 tests: guard normalization populates IR fields. Phase 6
    // wires up runtime evaluation; for now we only check the IR shape.

    #[test]
    fn p_match_should_populate_match_case_guard() {
        // match x { y where y => Nil _ => Nil } → MatchCase.guard is Some
        // for the first case, None for the wildcard.
        use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

        let par = ParBuilderUtil::mk_term("new x in { match *x { y where y => Nil _ => Nil } }")
            .expect("compile failed");
        // Walk the Par structure to find the Match. The outer New holds a
        // body Par with a single Match. Easier: print and assert via
        // structural checks.
        // We pull the Match out via the Par.matches slot inside the New
        // body.
        assert_eq!(par.news.len(), 1, "expected one New");
        let body = par.news[0].p.as_ref().expect("New.p missing");
        assert_eq!(body.matches.len(), 1, "expected one Match in New body");
        let m = &body.matches[0];
        assert_eq!(m.cases.len(), 2);
        assert!(
            m.cases[0].guard.is_some(),
            "first case should carry a guard"
        );
        assert!(m.cases[1].guard.is_none(), "wildcard case has no guard");
    }

    #[test]
    fn p_match_without_where_leaves_guard_none() {
        use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

        let par = ParBuilderUtil::mk_term("match 1 { 1 => Nil _ => Nil }").expect("compile failed");
        assert_eq!(par.matches.len(), 1);
        for case in &par.matches[0].cases {
            assert!(case.guard.is_none(), "no guard expected");
        }
    }

    #[test]
    fn p_input_should_populate_receive_condition() {
        // for (@x <- a where x) { Nil } → Receive.condition is Some.
        use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

        let par = ParBuilderUtil::mk_term("new a in { for (@x <- a where x) { Nil } }")
            .expect("compile failed");
        assert_eq!(par.news.len(), 1);
        let body = par.news[0].p.as_ref().expect("New.p missing");
        assert_eq!(body.receives.len(), 1);
        assert!(
            body.receives[0].condition.is_some(),
            "Receive.condition should be populated by `where` clause"
        );
    }

    #[test]
    fn p_input_without_where_leaves_condition_none() {
        use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

        let par =
            ParBuilderUtil::mk_term("new a in { for (@x <- a) { Nil } }").expect("compile failed");
        assert_eq!(par.news.len(), 1);
        let body = par.news[0].p.as_ref().expect("New.p missing");
        assert!(body.receives[0].condition.is_none());
    }

    #[test]
    fn p_input_join_with_where_attaches_to_receipt() {
        // for (@x <- a & @y <- b where x) { Nil } — join with single guard.
        use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

        let par =
            ParBuilderUtil::mk_term("new a, b in { for (@x <- a & @y <- b where x) { Nil } }")
                .expect("compile failed");
        let body = par.news[0].p.as_ref().expect("New.p missing");
        assert_eq!(body.receives.len(), 1);
        assert_eq!(body.receives[0].binds.len(), 2, "two &-joined binds");
        assert!(
            body.receives[0].condition.is_some(),
            "the guard attaches to the receipt as a whole"
        );
    }

    #[test]
    fn p_input_sequential_receipts_with_guards_nest() {
        // for (@x <- a where x; @y <- b where y) { Nil }
        // ; desugars to nested for, so we get an outer Receive with
        // condition Some(...) whose body is a Par containing an inner
        // Receive with its own condition.
        use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

        let par = ParBuilderUtil::mk_term(
            "new a, b in { for (@x <- a where x; @y <- b where y) { Nil } }",
        )
        .expect("compile failed");
        let body = par.news[0].p.as_ref().expect("New.p missing");
        assert_eq!(body.receives.len(), 1, "outer receive");
        let outer = &body.receives[0];
        assert!(outer.condition.is_some(), "outer guard");

        let inner_par = outer.body.as_ref().expect("outer body");
        assert_eq!(inner_par.receives.len(), 1, "inner receive (nested)");
        assert!(inner_par.receives[0].condition.is_some(), "inner guard");
    }

    // Phase 6: EMatchExpr classification + match-case fall-through.

    #[test]
    fn p_match_with_all_bool_bodies_emits_ematchexpr() {
        use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

        let par = ParBuilderUtil::mk_term("match 1 { 1 => true 2 => false _ => false }")
            .expect("compile failed");
        // Should be EMatchExpr in exprs, not Match in matches.
        assert_eq!(par.exprs.len(), 1, "expected one Expr");
        assert!(
            matches!(
                par.exprs[0].expr_instance,
                Some(models::rhoapi::expr::ExprInstance::EMatchExprBody(_))
            ),
            "match with all bool bodies should emit EMatchExpr; got {:?}",
            par.exprs[0].expr_instance
        );
        assert!(par.matches.is_empty(), "no Match in matches slot");
    }

    #[test]
    fn p_match_with_send_body_stays_match() {
        // match x { 1 => @"out"!(1) _ => @"out"!(0) } — bodies are sends,
        // not bool exprs, so we stay with process-level Match.
        use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

        let par = ParBuilderUtil::mk_term("match 1 { 1 => @\"out\"!(1) _ => @\"out\"!(0) }")
            .expect("compile failed");
        assert!(
            par.matches.len() == 1,
            "non-bool match should stay process-level"
        );
        assert!(
            par.exprs.is_empty()
                || !par.exprs.iter().any(|e| matches!(
                    e.expr_instance,
                    Some(models::rhoapi::expr::ExprInstance::EMatchExprBody(_))
                )),
            "should not emit EMatchExpr for non-bool body"
        );
    }

    #[test]
    fn p_match_case_with_guard_compiles() {
        // Just confirm match-case-with-where compiles without error and
        // produces a MatchCase whose guard is Some(_). Runtime
        // fall-through is exercised by the integration tests in
        // reduce_spec.rs.
        use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

        let par = ParBuilderUtil::mk_term("match 5 { n where n > 0 => true _ => false }")
            .expect("compile failed");
        // Bodies are bool, so this becomes EMatchExpr.
        assert_eq!(par.exprs.len(), 1);
        match &par.exprs[0].expr_instance {
            Some(models::rhoapi::expr::ExprInstance::EMatchExprBody(em)) => {
                assert_eq!(em.cases.len(), 2);
                assert!(em.cases[0].guard.is_some(), "first case guard");
                assert!(em.cases[1].guard.is_none(), "wildcard case has no guard");
            }
            other => panic!("expected EMatchExpr, got {other:?}"),
        }
    }
}
