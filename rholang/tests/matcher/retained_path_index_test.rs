use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{BindPattern, Expr, ListParWithRandom, Par};
use models::rust::utils::{
    new_elist_par, new_eset_par, new_freevar_par, new_freevar_var, new_wildcard_par,
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
