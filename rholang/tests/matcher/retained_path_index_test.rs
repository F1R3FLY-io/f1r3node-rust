use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{
    BindPattern, EAnd, EDiv, EEq, EGt, EGte, ELt, ELte, EMatches, EMethod, EMinus, EMinusMinus,
    EMod, EMult, ENeg, ENeq, ENot, EOr, EPercentPercent, EPlus, EPlusPlus, Expr, ListParWithRandom,
    Par,
};
use models::rust::utils::{
    new_elist_par, new_eplus_par, new_eset_par, new_freevar_par, new_freevar_var, new_wildcard_par,
};
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::rho_path::RhoPathEncode;

fn integer(value: i64) -> Par {
    let mut par = Par::default();
    par.exprs.push(Expr {
        expr_instance: Some(ExprInstance::GInt(value)),
    });
    par
}

fn datum(par: Par) -> ListParWithRandom {
    let mut datum = ListParWithRandom::default();
    datum.pars.push(par);
    datum
}

fn list(items: Vec<Par>, connective_used: bool) -> Par {
    new_elist_par(
        items,
        Vec::new(),
        connective_used,
        None,
        Vec::new(),
        connective_used,
    )
}

fn method(target: Par, argument: Par, connective_used: bool) -> Par {
    Par::default()
        .with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EMethodBody(EMethod {
                method_name: "lookup".to_string(),
                target: Some(target),
                arguments: vec![argument],
                locally_free: Vec::new(),
                connective_used,
            })),
        }])
        .with_connective_used(connective_used)
}

#[test]
fn exact_ground_pattern_and_datum_have_the_same_retained_path() {
    let matcher = Matcher;
    let value = integer(7);
    let pattern = BindPattern {
        patterns: vec![value.clone()],
        remainder: None,
        free_count: 0,
    };
    let datum = datum(value);

    assert_eq!(
        matcher.retained_pattern_path(&pattern),
        matcher.retained_datum_path(&datum)
    );
    assert!(matcher.get(&pattern, &datum).is_some());
}

#[test]
fn retained_paths_reject_binding_and_connective_patterns() {
    let matcher = Matcher;
    let mut binding = BindPattern {
        patterns: vec![integer(7)],
        remainder: None,
        free_count: 1,
    };
    assert!(matcher.retained_pattern_path(&binding).is_none());

    binding.free_count = 0;
    binding.patterns[0].connective_used = true;
    assert!(matcher.retained_pattern_path(&binding).is_none());
}

#[test]
fn distinct_ground_values_do_not_share_an_exact_retained_path() {
    let matcher = Matcher;
    let pattern = BindPattern {
        patterns: vec![integer(7)],
        remainder: None,
        free_count: 0,
    };
    let datum = datum(integer(8));

    assert_ne!(
        matcher.retained_pattern_path(&pattern),
        matcher.retained_datum_path(&datum)
    );
    assert!(matcher.get(&pattern, &datum).is_none());
}

#[test]
fn ordered_whole_term_variables_compile_to_the_same_query_used_by_rspace() {
    let matcher = Matcher;
    let pattern = BindPattern {
        patterns: vec![new_freevar_par(0, Vec::new()), integer(7)],
        remainder: None,
        free_count: 1,
    };
    let matching = ListParWithRandom {
        pars: vec![integer(11), integer(7)],
        ..ListParWithRandom::default()
    };
    let wrong_suffix = ListParWithRandom {
        pars: vec![integer(11), integer(8)],
        ..ListParWithRandom::default()
    };
    let wrong_arity = ListParWithRandom {
        pars: vec![integer(11), integer(7), integer(12)],
        ..ListParWithRandom::default()
    };
    let query = matcher
        .rho_path_query(&pattern)
        .expect("ordered whole-term pattern query");

    assert!(query.matches_structure(&matching.rho_path_structure()));
    assert!(!query.matches_structure(&wrong_suffix.rho_path_structure()));
    assert!(!query.matches_structure(&wrong_arity.rho_path_structure()));
    assert!(matcher.get(&pattern, &matching).is_some());
    assert!(matcher.get(&pattern, &wrong_suffix).is_none());
    assert!(matcher.get(&pattern, &wrong_arity).is_none());
}

#[test]
fn whole_term_wildcards_and_list_remainders_compile_without_an_exact_ground_fallback() {
    let matcher = Matcher;
    let wildcard = BindPattern {
        patterns: vec![new_wildcard_par(Vec::new(), true), integer(7)],
        remainder: Some(new_freevar_var(0)),
        free_count: 1,
    };
    let matching = ListParWithRandom {
        pars: vec![integer(11), integer(7), integer(12), integer(13)],
        ..ListParWithRandom::default()
    };
    let too_short = ListParWithRandom {
        pars: vec![integer(11)],
        ..ListParWithRandom::default()
    };
    let query = matcher
        .rho_path_query(&wildcard)
        .expect("wildcard and remainder query");

    assert!(matcher.retained_pattern_path(&wildcard).is_none());
    assert!(query.matches_structure(&matching.rho_path_structure()));
    assert!(!query.matches_structure(&too_short.rho_path_structure()));
    assert!(matcher.get(&wildcard, &matching).is_some());
    assert!(matcher.get(&wildcard, &too_short).is_none());

    let invalid_remainder = BindPattern {
        remainder: Some(models::rhoapi::Var {
            var_instance: Some(VarInstance::BoundVar(0)),
        }),
        ..wildcard
    };
    assert!(matcher.rho_path_query(&invalid_remainder).is_none());
}

#[test]
fn nested_ordered_variables_compile_to_structural_queries() {
    let matcher = Matcher;
    let pattern = BindPattern {
        patterns: vec![list(vec![new_freevar_par(0, Vec::new()), integer(7)], true)],
        remainder: None,
        free_count: 1,
    };
    let matching = datum(list(vec![integer(11), integer(7)], false));
    let wrong_suffix = datum(list(vec![integer(11), integer(8)], false));
    let wrong_arity = datum(list(vec![integer(11)], false));
    let query = matcher
        .rho_path_query(&pattern)
        .expect("nested ordered pattern query");

    assert!(query.matches_structure(&matching.rho_path_structure()));
    assert!(!query.matches_structure(&wrong_suffix.rho_path_structure()));
    assert!(!query.matches_structure(&wrong_arity.rho_path_structure()));
    assert!(matcher.get(&pattern, &matching).is_some());
    assert!(matcher.get(&pattern, &wrong_suffix).is_none());
    assert!(matcher.get(&pattern, &wrong_arity).is_none());
}

#[test]
fn nonlinear_ordered_bindings_fall_back_before_a_query_can_change_legacy_semantics() {
    let matcher = Matcher;
    let pattern = BindPattern {
        patterns: vec![new_eplus_par(
            new_freevar_par(0, Vec::new()),
            new_freevar_par(0, Vec::new()),
        )],
        remainder: None,
        free_count: 1,
    };
    let matching = datum(new_eplus_par(integer(11), integer(11)));
    let conflicting = datum(new_eplus_par(integer(11), integer(12)));
    assert!(matcher.rho_path_query(&pattern).is_none());
    assert!(matcher.get(&pattern, &matching).is_some());
    assert!(
        matcher.get(&pattern, &conflicting).is_some(),
        "the legacy matcher accumulates nonlinear free-variable occurrences; the derived query must not reinterpret them as equality constraints"
    );

    let repeated_whole_terms = BindPattern {
        patterns: vec![
            new_freevar_par(0, Vec::new()),
            new_freevar_par(0, Vec::new()),
        ],
        remainder: None,
        free_count: 1,
    };
    assert!(matcher.rho_path_query(&repeated_whole_terms).is_none());

    let repeated_across_atoms = BindPattern {
        patterns: vec![
            new_freevar_par(0, Vec::new()),
            method(new_freevar_par(0, Vec::new()), integer(7), true),
        ],
        remainder: None,
        free_count: 1,
    };
    assert!(matcher.rho_path_query(&repeated_across_atoms).is_none());
}

#[test]
fn ordered_method_target_and_arguments_compile_without_a_variant_specific_path() {
    let matcher = Matcher;
    let pattern = BindPattern {
        patterns: vec![method(new_freevar_par(0, Vec::new()), integer(7), true)],
        remainder: None,
        free_count: 1,
    };
    let matching = datum(method(integer(11), integer(7), false));
    let wrong_argument = datum(method(integer(11), integer(8), false));
    let query = matcher
        .rho_path_query(&pattern)
        .expect("ordered method query");

    assert!(query.matches_structure(&matching.rho_path_structure()));
    assert!(!query.matches_structure(&wrong_argument.rho_path_structure()));
    assert!(matcher.get(&pattern, &matching).is_some());
    assert!(matcher.get(&pattern, &wrong_argument).is_none());
}

#[test]
fn every_ordered_binary_expression_uses_the_same_generated_query_path() {
    let matcher = Matcher;
    let mut cases = Vec::new();
    macro_rules! push_case {
        ($name:literal, $variant:ident, $body:ident) => {{
            let expression = |left: Par, right: Par, connective_used: bool| {
                Par::default()
                    .with_exprs(vec![Expr {
                        expr_instance: Some(ExprInstance::$variant($body {
                            p1: Some(left),
                            p2: Some(right),
                        })),
                    }])
                    .with_connective_used(connective_used)
            };
            cases.push((
                $name,
                expression(new_freevar_par(0, Vec::new()), integer(7), true),
                expression(integer(11), integer(7), false),
                expression(integer(11), integer(8), false),
            ));
        }};
    }
    push_case!("EMult", EMultBody, EMult);
    push_case!("EDiv", EDivBody, EDiv);
    push_case!("EMod", EModBody, EMod);
    push_case!("EPercentPercent", EPercentPercentBody, EPercentPercent);
    push_case!("EPlus", EPlusBody, EPlus);
    push_case!("EPlusPlus", EPlusPlusBody, EPlusPlus);
    push_case!("EMinusMinus", EMinusMinusBody, EMinusMinus);
    push_case!("EMinus", EMinusBody, EMinus);
    push_case!("ELt", ELtBody, ELt);
    push_case!("ELte", ELteBody, ELte);
    push_case!("EGt", EGtBody, EGt);
    push_case!("EGte", EGteBody, EGte);
    push_case!("EEq", EEqBody, EEq);
    push_case!("ENeq", ENeqBody, ENeq);
    push_case!("EAnd", EAndBody, EAnd);
    push_case!("EOr", EOrBody, EOr);

    for (name, pattern, matching, wrong_rhs) in cases {
        let pattern = BindPattern {
            patterns: vec![pattern],
            remainder: None,
            free_count: 1,
        };
        let matching = datum(matching);
        let wrong_rhs = datum(wrong_rhs);
        let query = matcher
            .rho_path_query(&pattern)
            .unwrap_or_else(|| panic!("{name}: ordered generated query"));
        assert!(
            query.matches_structure(&matching.rho_path_structure()),
            "{name}"
        );
        assert!(
            !query.matches_structure(&wrong_rhs.rho_path_structure()),
            "{name}"
        );
        assert!(matcher.get(&pattern, &matching).is_some(), "{name}");
        assert!(matcher.get(&pattern, &wrong_rhs).is_none(), "{name}");
    }
}

#[test]
fn ordered_unary_and_matches_expressions_use_the_generated_query_path() {
    let matcher = Matcher;
    let not = |value: Par, connective_used: bool| {
        Par::default()
            .with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::ENotBody(ENot { p: Some(value) })),
            }])
            .with_connective_used(connective_used)
    };
    let neg = |value: Par, connective_used: bool| {
        Par::default()
            .with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::ENegBody(ENeg { p: Some(value) })),
            }])
            .with_connective_used(connective_used)
    };
    let matches = |target: Par, exact_pattern: Par, connective_used: bool| {
        Par::default()
            .with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::EMatchesBody(EMatches {
                    target: Some(target),
                    pattern: Some(exact_pattern),
                })),
            }])
            .with_connective_used(connective_used)
    };
    let cases = [
        (
            "ENot",
            not(new_freevar_par(0, Vec::new()), true),
            not(integer(11), false),
            neg(integer(12), false),
        ),
        (
            "ENeg",
            neg(new_freevar_par(0, Vec::new()), true),
            neg(integer(11), false),
            not(integer(12), false),
        ),
        (
            "EMatches",
            matches(new_freevar_par(0, Vec::new()), integer(7), true),
            matches(integer(11), integer(7), false),
            matches(integer(11), integer(8), false),
        ),
    ];

    for (name, pattern, matching, nonmatching) in cases {
        let pattern = BindPattern {
            patterns: vec![pattern],
            remainder: None,
            free_count: 1,
        };
        let matching = datum(matching);
        let nonmatching = datum(nonmatching);
        let query = matcher
            .rho_path_query(&pattern)
            .unwrap_or_else(|| panic!("{name}: ordered generated query"));
        assert!(
            query.matches_structure(&matching.rho_path_structure()),
            "{name}"
        );
        assert!(
            !query.matches_structure(&nonmatching.rho_path_structure()),
            "{name}"
        );
        assert!(matcher.get(&pattern, &matching).is_some(), "{name}");
        assert!(matcher.get(&pattern, &nonmatching).is_none(), "{name}");
    }
}

#[test]
fn unordered_nested_variables_remain_on_the_spatial_fallback() {
    let matcher = Matcher;
    let pattern = BindPattern {
        patterns: vec![new_eset_par(
            vec![integer(7), new_freevar_par(0, Vec::new())],
            Vec::new(),
            true,
            None,
            Vec::new(),
            true,
        )],
        remainder: None,
        free_count: 1,
    };
    assert!(matcher.rho_path_query(&pattern).is_none());
}
