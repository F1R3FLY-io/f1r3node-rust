use std::collections::BTreeSet;
use std::sync::Arc;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{
    BindPattern, EAnd, EDiv, EEq, EGt, EGte, ELt, ELte, EMatches, EMethod, EMinus, EMinusMinus,
    EMod, EMult, ENeg, ENeq, ENot, EOr, EPercentPercent, EPlus, EPlusPlus, Expr, ListParWithRandom,
    Par, TaggedContinuation,
};
use models::rust::utils::{
    new_elist_par, new_eplus_par, new_eset_par, new_freevar_par, new_freevar_var, new_wildcard_par,
};
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::replay_rspace::ReplayRSpace;
use rspace_plus_plus::rspace::rho_path::RhoPathEncode;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::{ISpace, MaybeProduceResult};
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

fn integer(value: i64) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(value)),
    }])
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

    let repeated_method = BindPattern {
        patterns: vec![method(
            new_freevar_par(0, Vec::new()),
            new_freevar_par(0, Vec::new()),
            true,
        )],
        remainder: None,
        free_count: 1,
    };
    let conflicting_method = datum(method(integer(11), integer(12), false));
    assert!(matcher.rho_path_query(&repeated_method).is_none());
    assert!(matcher.get(&repeated_method, &conflicting_method).is_some());

    let repeated_whole_terms = BindPattern {
        patterns: vec![
            new_freevar_par(0, Vec::new()),
            new_freevar_par(0, Vec::new()),
        ],
        remainder: None,
        free_count: 1,
    };
    assert!(matcher.rho_path_query(&repeated_whole_terms).is_none());
    let conflicting_whole_terms = ListParWithRandom {
        pars: vec![integer(11), integer(12)],
        ..ListParWithRandom::default()
    };
    assert!(matcher
        .get(&repeated_whole_terms, &conflicting_whole_terms)
        .is_some());

    let repeated_across_atoms = BindPattern {
        patterns: vec![
            new_freevar_par(0, Vec::new()),
            method(new_freevar_par(0, Vec::new()), integer(7), true),
        ],
        remainder: None,
        free_count: 1,
    };
    assert!(matcher.rho_path_query(&repeated_across_atoms).is_none());
    let conflicting_across_atoms = ListParWithRandom {
        pars: vec![integer(11), method(integer(12), integer(7), false)],
        ..ListParWithRandom::default()
    };
    assert!(matcher
        .get(&repeated_across_atoms, &conflicting_across_atoms)
        .is_some());
}

#[test]
fn deep_region_query_scanner_is_stack_safe_for_a_dynamic_unary_chain() {
    std::thread::Builder::new()
        .name("deep-rmp2-region-query".to_string())
        .stack_size(512 * 1024)
        .spawn(|| {
            const DEPTH: usize = 4_096;
            let matcher = Matcher;
            let pattern = BindPattern {
                patterns: vec![Par::wrap_unary_elist(DEPTH, new_freevar_par(0, Vec::new()))
                    .expect("canonical unary-list pattern is representable")],
                remainder: None,
                free_count: 1,
            };
            let query = matcher
                .rho_path_query(&pattern)
                .expect("deep linear capture has a conservative RMP2 query");
            let matching = datum(
                Par::wrap_unary_elist(DEPTH, integer(99))
                    .expect("canonical unary-list datum is representable"),
            );
            assert!(query.matches_structure(&matching.rho_path_structure()));
        })
        .unwrap()
        .join()
        .unwrap();
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

#[derive(Clone)]
struct GeneratedRSpaceCase {
    name: &'static str,
    pattern: BindPattern,
    matching: ListParWithRandom,
    nonmatching: [ListParWithRandom; 2],
}

fn generated_rspace_cases() -> Vec<GeneratedRSpaceCase> {
    let mut cases = Vec::new();
    macro_rules! push_binary_case {
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
            cases.push(GeneratedRSpaceCase {
                name: $name,
                pattern: BindPattern {
                    patterns: vec![expression(new_freevar_par(0, Vec::new()), integer(7), true)],
                    remainder: None,
                    free_count: 1,
                },
                matching: datum(expression(integer(11), integer(7), false)),
                nonmatching: [
                    datum(expression(integer(11), integer(8), false)),
                    datum(expression(integer(12), integer(9), false)),
                ],
            });
        }};
    }
    push_binary_case!("EMult", EMultBody, EMult);
    push_binary_case!("EDiv", EDivBody, EDiv);
    push_binary_case!("EMod", EModBody, EMod);
    push_binary_case!("EPercentPercent", EPercentPercentBody, EPercentPercent);
    push_binary_case!("EPlus", EPlusBody, EPlus);
    push_binary_case!("EPlusPlus", EPlusPlusBody, EPlusPlus);
    push_binary_case!("EMinusMinus", EMinusMinusBody, EMinusMinus);
    push_binary_case!("EMinus", EMinusBody, EMinus);
    push_binary_case!("ELt", ELtBody, ELt);
    push_binary_case!("ELte", ELteBody, ELte);
    push_binary_case!("EGt", EGtBody, EGt);
    push_binary_case!("EGte", EGteBody, EGte);
    push_binary_case!("EEq", EEqBody, EEq);
    push_binary_case!("ENeq", ENeqBody, ENeq);
    push_binary_case!("EAnd", EAndBody, EAnd);
    push_binary_case!("EOr", EOrBody, EOr);

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
    cases.push(GeneratedRSpaceCase {
        name: "ENot",
        pattern: BindPattern {
            patterns: vec![not(new_freevar_par(0, Vec::new()), true)],
            remainder: None,
            free_count: 1,
        },
        matching: datum(not(integer(11), false)),
        nonmatching: [datum(neg(integer(11), false)), datum(integer(8))],
    });
    cases.push(GeneratedRSpaceCase {
        name: "ENeg",
        pattern: BindPattern {
            patterns: vec![neg(new_freevar_par(0, Vec::new()), true)],
            remainder: None,
            free_count: 1,
        },
        matching: datum(neg(integer(11), false)),
        nonmatching: [datum(not(integer(11), false)), datum(integer(9))],
    });

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
    cases.push(GeneratedRSpaceCase {
        name: "EMatches",
        pattern: BindPattern {
            patterns: vec![matches(new_freevar_par(0, Vec::new()), integer(7), true)],
            remainder: None,
            free_count: 1,
        },
        matching: datum(matches(integer(11), integer(7), false)),
        nonmatching: [
            datum(matches(integer(11), integer(8), false)),
            datum(matches(integer(12), integer(9), false)),
        ],
    });
    cases.push(GeneratedRSpaceCase {
        name: "EMethod",
        pattern: BindPattern {
            patterns: vec![method(new_freevar_par(0, Vec::new()), integer(7), true)],
            remainder: None,
            free_count: 1,
        },
        matching: datum(method(integer(11), integer(7), false)),
        nonmatching: [
            datum(method(integer(11), integer(8), false)),
            datum(method(integer(12), integer(9), false)),
        ],
    });
    cases.push(GeneratedRSpaceCase {
        name: "EList",
        pattern: BindPattern {
            patterns: vec![list(vec![new_freevar_par(0, Vec::new()), integer(7)], true)],
            remainder: None,
            free_count: 1,
        },
        matching: datum(list(vec![integer(11), integer(7)], false)),
        nonmatching: [
            datum(list(vec![integer(11), integer(8)], false)),
            datum(list(vec![integer(12), integer(9)], false)),
        ],
    });
    cases
}

#[derive(Clone, Default)]
struct FullScanMatcher;

impl Match<BindPattern, ListParWithRandom, TaggedContinuation> for FullScanMatcher {
    fn get(&self, pattern: &BindPattern, datum: &ListParWithRandom) -> Option<ListParWithRandom> {
        Matcher.get(pattern, datum)
    }

    fn check_commit(
        &self,
        continuation: &TaggedContinuation,
        matched: &[&ListParWithRandom],
    ) -> bool {
        Matcher.check_commit(continuation, matched)
    }
}

type RhoSpace = RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;
type RhoReplay = ReplayRSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

async fn space_pair<M>(matcher: M) -> (RhoSpace, RhoReplay)
where M: Match<BindPattern, ListParWithRandom, TaggedContinuation> + 'static {
    let mut manager = InMemoryStoreManager::new();
    let stores = manager
        .r_space_stores()
        .await
        .expect("in-memory RSpace stores");
    RSpace::create_with_replay(stores, Arc::new(Box::new(matcher)))
        .expect("play/replay RSpace pair")
}

async fn run_generated_workload<S>(
    space: &S,
    cases: &[GeneratedRSpaceCase],
) -> Vec<MaybeProduceResult<Par, BindPattern, ListParWithRandom, TaggedContinuation>>
where
    S: ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
{
    let mut results = Vec::with_capacity(cases.len());
    for (index, case) in cases.iter().enumerate() {
        let channel = integer(10_000 + i64::try_from(index).expect("bounded case index"));
        for nonmatching in &case.nonmatching {
            assert!(
                space
                    .produce(channel.clone(), nonmatching.clone(), false)
                    .await
                    .unwrap_or_else(|error| panic!("{}: nonmatching produce: {error}", case.name))
                    .is_none(),
                "{}: a nonmatching datum cannot fire",
                case.name
            );
        }
        assert!(
            space
                .consume(
                    vec![channel.clone()],
                    vec![case.pattern.clone()],
                    TaggedContinuation::default(),
                    false,
                    BTreeSet::new(),
                )
                .await
                .unwrap_or_else(|error| panic!("{}: consume: {error}", case.name))
                .is_none(),
            "{}: the receive must wait after both negative controls",
            case.name
        );
        let result = space
            .produce(channel, case.matching.clone(), false)
            .await
            .unwrap_or_else(|error| panic!("{}: matching produce: {error}", case.name));
        assert!(result.is_some(), "{}: matching datum must fire", case.name);
        results.push(result);
    }
    results
}

#[tokio::test]
async fn generated_ordered_queries_preserve_play_replay_results_logs_and_roots() {
    let cases = generated_rspace_cases();
    assert_eq!(
        cases.len(),
        21,
        "the generated ordered eligibility corpus changed"
    );

    let (control, control_replay) = space_pair(FullScanMatcher).await;
    let (indexed, indexed_replay) = space_pair(Matcher).await;
    let control_empty = control
        .create_checkpoint()
        .await
        .expect("control empty checkpoint");
    let indexed_empty = indexed
        .create_checkpoint()
        .await
        .expect("indexed empty checkpoint");
    assert_eq!(control_empty.root, indexed_empty.root, "empty roots");

    let control_results = run_generated_workload(&control, &cases).await;
    let indexed_results = run_generated_workload(&indexed, &cases).await;
    assert_eq!(indexed_results, control_results, "play results");
    assert_eq!(
        indexed.to_map().await,
        control.to_map().await,
        "play hot stores"
    );

    let control_point = control
        .create_checkpoint()
        .await
        .expect("control checkpoint");
    let indexed_point = indexed
        .create_checkpoint()
        .await
        .expect("indexed checkpoint");
    assert_eq!(indexed_point.log, control_point.log, "play event histories");
    assert_eq!(
        indexed_point.root, control_point.root,
        "play post-state roots"
    );

    control_replay
        .rig_and_reset(control_empty.root, control_point.log.clone())
        .await
        .expect("rig control replay");
    indexed_replay
        .rig_and_reset(indexed_empty.root, indexed_point.log.clone())
        .await
        .expect("rig indexed replay");
    let control_replay_results = run_generated_workload(&control_replay, &cases).await;
    let indexed_replay_results = run_generated_workload(&indexed_replay, &cases).await;
    assert_eq!(
        control_replay_results, control_results,
        "control replay results"
    );
    assert_eq!(
        indexed_replay_results, indexed_results,
        "indexed replay results"
    );
    assert_eq!(
        indexed_replay.to_map().await,
        control_replay.to_map().await,
        "replay hot stores"
    );

    let control_replay_point = control_replay
        .create_checkpoint()
        .await
        .expect("control replay checkpoint");
    let indexed_replay_point = indexed_replay
        .create_checkpoint()
        .await
        .expect("indexed replay checkpoint");
    assert_eq!(
        control_replay_point.root, control_point.root,
        "control replay root"
    );
    assert_eq!(
        indexed_replay_point.root, indexed_point.root,
        "indexed replay root"
    );
    control_replay
        .check_replay_data()
        .await
        .expect("control rig exhausted");
    indexed_replay
        .check_replay_data()
        .await
        .expect("indexed rig exhausted");
}
