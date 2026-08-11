//! Differential between the production PDA and the exact pre-PDA matcher.

use std::collections::BTreeMap;

use models::rhoapi::connective::ConnectiveInstance::{ConnAndBody, ConnNotBody, ConnOrBody};
use models::rhoapi::expr::ExprInstance::EPathmapBody;
use models::rhoapi::{
    Connective, ConnectiveBody, EPathMap, ETuple, Expr, KeyValuePair, New, Par, Receive,
    ReceiveBind, Send,
};
use models::rust::utils::{
    new_elist_par, new_emap_par, new_eset_par, new_etuple_par, new_freevar_expr, new_freevar_par,
    new_gint_par, new_gstring_par, new_wildcard_par,
};

use super::recursive_oracle::spatial_matcher::{
    SpatialMatcher as RecursiveSpatialMatcher, SpatialMatcherContext as RecursiveContext,
};
use super::spatial_matcher::{SpatialMatcher, SpatialMatcherContext};
use super::spatial_matcher_pda::match_par_list_with_stats;

fn compare(label: &str, target: Par, pattern: Par) {
    let mut recursive = RecursiveContext::new();
    let recursive_verdict = recursive
        .spatial_match(target.clone(), pattern.clone())
        .is_some();
    let recursive_map = recursive.free_map;

    let mut pda = SpatialMatcherContext::new();
    let pda_verdict = pda.spatial_match(target, pattern).is_some();
    assert_eq!(pda_verdict, recursive_verdict, "{label}: verdict");
    assert_eq!(pda.free_map, recursive_map, "{label}: FreeMap");
}

fn list(ps: Vec<Par>, connective_used: bool) -> Par {
    new_elist_par(
        ps,
        Vec::new(),
        connective_used,
        None,
        Vec::new(),
        connective_used,
    )
}

fn tuple(ps: Vec<Par>, connective_used: bool) -> Par {
    models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(models::rhoapi::expr::ExprInstance::ETupleBody(
                ETuple {
                    ps,
                    locally_free: Vec::new(),
                    connective_used,
                },
            )),
        }],
        connective_used,
        ..Default::default()
    }
}

fn connective(instance: models::rhoapi::connective::ConnectiveInstance) -> Par {
    models::par_from_default! {
        connectives: vec![Connective {
            connective_instance: Some(instance),
        }],
        connective_used: true,
        ..Default::default()
    }
}

#[derive(Clone, Copy)]
struct NominalFrame {
    pattern: usize,
    next_target: usize,
    pending_target: Option<usize>,
}

/// Exact pre-reuse Kuhn work counter. This oracle owns no relation cache and
/// is intentionally separate from the production PDA.
fn nominal_edge_evaluations(
    pattern_count: usize,
    target_count: usize,
    edge: impl Fn(usize, usize) -> bool,
) -> (bool, usize) {
    let mut assignments = vec![None; target_count];
    let mut evaluations = 0;

    for root_pattern in 0..pattern_count {
        let mut seen_targets = vec![false; target_count];
        let mut search = vec![NominalFrame {
            pattern: root_pattern,
            next_target: 0,
            pending_target: None,
        }];
        let mut augmented = false;

        loop {
            let Some(frame) = search.last_mut() else {
                break;
            };
            let mut descended = false;
            while frame.next_target < target_count {
                let target = frame.next_target;
                frame.next_target += 1;
                if seen_targets[target] {
                    continue;
                }
                evaluations += 1;
                if !edge(frame.pattern, target) {
                    continue;
                }
                seen_targets[target] = true;
                match assignments[target] {
                    None => {
                        assignments[target] = Some(frame.pattern);
                        search.pop();
                        while let Some(parent) = search.pop() {
                            assignments[parent
                                .pending_target
                                .expect("a nominal parent carries its displaced edge")] =
                                Some(parent.pattern);
                        }
                        augmented = true;
                        break;
                    }
                    Some(previous_pattern) => {
                        frame.pending_target = Some(target);
                        search.push(NominalFrame {
                            pattern: previous_pattern,
                            next_target: 0,
                            pending_target: None,
                        });
                        descended = true;
                        break;
                    }
                }
            }
            if augmented {
                break;
            }
            if descended {
                continue;
            }
            search.pop();
            if let Some(parent) = search.last_mut() {
                parent.pending_target = None;
            } else {
                break;
            }
        }
        if !augmented {
            return (false, evaluations);
        }
    }

    (true, evaluations)
}

fn send(chan: Par, data: Vec<Par>, connective_used: bool) -> Par {
    models::par_from_default! {
        sends: vec![Send {
            chan: Some(chan),
            data,
            persistent: false,
            locally_free: Vec::new(),
            connective_used,
        }],
        connective_used,
        ..Default::default()
    }
}

fn receive(source: Par, body: Par, connective_used: bool) -> Par {
    models::par_from_default! {
        receives: vec![Receive {
            binds: vec![ReceiveBind {
                patterns: vec![Par::default()],
                source: Some(source),
                remainder: None,
                free_count: 0,
            }],
            body: Some(body),
            persistent: false,
            peek: false,
            bind_count: 1,
            locally_free: Vec::new(),
            connective_used,
            condition: None,
        }],
        connective_used,
        ..Default::default()
    }
}

fn new_scope(body: Par, connective_used: bool) -> Par {
    models::par_from_default! {
        news: vec![New {
            bind_count: 1,
            p: Some(body),
            uri: Vec::new(),
            injections: BTreeMap::new(),
            locally_free: Vec::new(),
        }],
        connective_used,
        ..Default::default()
    }
}

fn pathmap(map: EPathMap, connective_used: bool) -> Par {
    models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(EPathmapBody(map)),
        }],
        connective_used,
        ..Default::default()
    }
}

#[test]
fn recursive_oracle_and_pda_agree_on_the_semantic_corpus() {
    let one = || new_gint_par(1, Vec::new(), false);
    let two = || new_gint_par(2, Vec::new(), false);
    let seven = || new_gint_par(7, Vec::new(), false);
    let free = |level| new_freevar_par(level, Vec::new());

    compare("ground equality", one(), one());
    compare("ground refusal", one(), two());
    compare("free capture", seven(), free(0));
    compare("wildcard", seven(), new_wildcard_par(Vec::new(), true));

    for depth in 0..=8 {
        let mut target = seven();
        let mut pattern = free(0);
        for _ in 0..depth {
            target = list(vec![target], false);
            pattern = list(vec![pattern], true);
        }
        compare(&format!("nested list depth {depth}"), target, pattern);
    }

    compare(
        "tuple fields",
        new_etuple_par(vec![one(), two()]),
        tuple(vec![free(0), free(1)], true),
    );
    compare(
        "send channel and data",
        send(one(), vec![seven()], false),
        send(free(0), vec![free(1)], true),
    );
    let duplicate = send(one(), vec![seven()], false).sends[0].clone();
    let duplicate_target = models::par_from_default! {
        sends: vec![duplicate.clone(), duplicate.clone(), duplicate],
        ..Default::default()
    };
    let mut duplicate_pattern = send(one(), vec![free(0)], true);
    duplicate_pattern.exprs.push(new_freevar_expr(1));
    duplicate_pattern.connective_used = true;
    compare(
        "Par remainder over byte-identical sends",
        duplicate_target,
        duplicate_pattern,
    );
    compare(
        "receive source and body",
        receive(one(), seven(), false),
        receive(free(0), free(1), true),
    );
    compare(
        "new body",
        new_scope(seven(), false),
        new_scope(free(0), true),
    );

    compare(
        "conjunction",
        seven(),
        connective(ConnAndBody(ConnectiveBody {
            ps: vec![free(0), seven()],
        })),
    );
    compare(
        "disjunction",
        seven(),
        connective(ConnOrBody(ConnectiveBody {
            ps: vec![two(), free(0)],
        })),
    );
    compare("negation success", seven(), connective(ConnNotBody(two())));
    compare(
        "negation refusal",
        seven(),
        connective(ConnNotBody(free(0))),
    );

    compare(
        "legacy set",
        new_eset_par(
            vec![one(), two()],
            Vec::new(),
            false,
            None,
            Vec::new(),
            false,
        ),
        new_eset_par(
            vec![one(), free(0)],
            Vec::new(),
            true,
            None,
            Vec::new(),
            true,
        ),
    );
    compare(
        "legacy map",
        new_emap_par(
            vec![KeyValuePair {
                key: Some(one()),
                value: Some(seven()),
            }],
            Vec::new(),
            false,
            None,
            Vec::new(),
            false,
        ),
        new_emap_par(
            vec![KeyValuePair {
                key: Some(one()),
                value: Some(free(0)),
            }],
            Vec::new(),
            true,
            None,
            Vec::new(),
            true,
        ),
    );

    let mut target_set = EPathMap::default();
    target_set.insert_entry(one());
    target_set.insert_entry(two());
    let mut pattern_set = EPathMap::default();
    pattern_set.insert_entry(one());
    pattern_set.insert_entry(free(0));
    pattern_set.connective_used = true;
    compare(
        "PathMap set",
        pathmap(target_set, false),
        pathmap(pattern_set, true),
    );

    let mut target_map = EPathMap::default();
    target_map.insert_map_entry(one(), seven()).unwrap();
    let mut pattern_map = EPathMap::default();
    pattern_map.insert_map_entry(one(), free(0)).unwrap();
    pattern_map.connective_used = true;
    compare(
        "PathMap map",
        pathmap(target_map, false),
        pathmap(pattern_map, true),
    );

    compare(
        "empty PathMap",
        pathmap(EPathMap::default(), false),
        pathmap(EPathMap::default(), false),
    );

    let singleton_set = |member: Par| {
        let mut map = EPathMap::default();
        map.insert_entry(member);
        map
    };
    compare(
        "concrete singleton PathMap set",
        pathmap(singleton_set(one()), false),
        pathmap(singleton_set(one()), false),
    );
    compare(
        "concrete singleton PathMap set mismatch",
        pathmap(singleton_set(one()), false),
        pathmap(singleton_set(two()), false),
    );

    let singleton_map = |key: Par, value: Par| {
        let mut map = EPathMap::default();
        map.insert_map_entry(key, value).unwrap();
        map
    };
    compare(
        "concrete singleton PathMap map",
        pathmap(singleton_map(one(), seven()), false),
        pathmap(singleton_map(one(), seven()), false),
    );
    compare(
        "concrete singleton PathMap map key mismatch",
        pathmap(singleton_map(one(), seven()), false),
        pathmap(singleton_map(two(), seven()), false),
    );
    compare(
        "concrete singleton PathMap map value mismatch",
        pathmap(singleton_map(one(), seven()), false),
        pathmap(singleton_map(one(), two()), false),
    );

    compare(
        "string mismatch",
        new_gstring_par("a".into(), Vec::new(), false),
        new_gstring_par("b".into(), Vec::new(), false),
    );
}

#[test]
fn production_list_pda_preserves_the_attempt_baseline_when_committing_deltas() {
    let baseline_value = new_gstring_par("baseline".into(), Vec::new(), false);
    let first = new_gint_par(11, Vec::new(), false);
    let second = new_gint_par(29, Vec::new(), false);
    let mut context = SpatialMatcherContext::new();
    context.free_map.insert(41, baseline_value.clone());

    let (result, _) =
        match_par_list_with_stats(&mut context, vec![first.clone(), second.clone()], vec![
            first,
            new_freevar_par(0, Vec::new()),
        ]);

    assert!(result.is_some());
    assert_eq!(context.free_map.len(), 2);
    assert_eq!(context.free_map.get(&41), Some(&baseline_value));
    assert_eq!(context.free_map.get(&0), Some(&second));
}

#[test]
fn production_list_pda_matches_the_oracle_across_orderings_duplicates_and_nonlinearity() {
    const PERMUTATIONS: [[usize; 3]; 6] =
        [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [
            2, 1, 0,
        ]];

    let integer = |value| new_gint_par(value, Vec::new(), false);
    for target_values in [[1, 2, 3], [1, 1, 2]] {
        for target_order in PERMUTATIONS {
            let targets = target_order
                .map(|index| integer(target_values[index]))
                .into_iter()
                .collect::<Vec<_>>();
            for pattern_order in PERMUTATIONS {
                let base_patterns = [
                    new_freevar_par(0, Vec::new()),
                    integer(2),
                    new_freevar_par(1, Vec::new()),
                ];
                let patterns = pattern_order
                    .map(|index| base_patterns[index].clone())
                    .into_iter()
                    .collect::<Vec<_>>();
                compare(
                    &format!(
                        "AC ordering target={target_order:?} pattern={pattern_order:?} values={target_values:?}"
                    ),
                    list(targets.clone(), false),
                    list(patterns, true),
                );

                let nonlinear_base = [
                    new_freevar_par(0, Vec::new()),
                    integer(2),
                    new_freevar_par(0, Vec::new()),
                ];
                let nonlinear = pattern_order
                    .map(|index| nonlinear_base[index].clone())
                    .into_iter()
                    .collect::<Vec<_>>();
                compare(
                    &format!(
                        "nonlinear AC refusal target={target_order:?} pattern={pattern_order:?} values={target_values:?}"
                    ),
                    list(targets.clone(), false),
                    list(nonlinear, true),
                );
            }
        }
    }
}

#[test]
fn production_list_pda_relational_counters_meet_preregistered_controls() {
    const WIDTH: usize = 128;
    const SAMPLES: usize = 51;

    let integers = || {
        (0..WIDTH)
            .map(|value| new_gint_par(value as i64, Vec::new(), false))
            .collect::<Vec<_>>()
    };

    let diagonal_patterns = integers();
    let (diagonal_verdict, diagonal_control) =
        nominal_edge_evaluations(WIDTH, WIDTH, |pattern, target| pattern == target);
    let mut diagonal_context = SpatialMatcherContext::new();
    let (diagonal_result, diagonal_stats) =
        match_par_list_with_stats(&mut diagonal_context, integers(), diagonal_patterns);
    assert!(diagonal_verdict);
    assert!(diagonal_result.is_some());
    assert_eq!(diagonal_stats.edge_evaluations, diagonal_control);
    assert_eq!(diagonal_stats.relation_row_reuses, 0);
    assert_eq!(diagonal_stats.cached_edge_visits, 0);

    let chain_edge = |pattern: usize, target: usize| {
        if pattern == 0 {
            target == 1
        } else {
            target == pattern || target == pattern + 1
        }
    };
    let (chain_verdict, chain_control) = nominal_edge_evaluations(WIDTH, WIDTH, chain_edge);
    assert!(
        !chain_verdict,
        "target zero deliberately has no incoming edge"
    );

    let chain_patterns = || {
        (0..WIDTH)
            .map(|pattern| {
                if pattern == 0 {
                    new_gint_par(1, Vec::new(), false)
                } else {
                    connective(ConnOrBody(ConnectiveBody {
                        ps: vec![
                            new_gint_par(pattern as i64, Vec::new(), false),
                            new_gint_par((pattern + 1) as i64, Vec::new(), false),
                        ],
                    }))
                }
            })
            .collect::<Vec<_>>()
    };

    let mut control_samples = Vec::with_capacity(SAMPLES);
    let mut treatment_samples = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let mut context = SpatialMatcherContext::new();
        let (result, stats) = match_par_list_with_stats(&mut context, integers(), chain_patterns());
        assert!(result.is_none());
        assert_eq!(stats.edge_evaluations, WIDTH * WIDTH);
        assert!(stats.relation_row_reuses > 0);
        assert!(stats.cached_edge_visits > 0);
        assert!(stats.successful_edge_evaluations <= 2 * WIDTH);
        assert!(stats.edge_evaluations < chain_control);
        control_samples.push(chain_control);
        treatment_samples.push(stats.edge_evaluations);
    }

    println!(
        "D_E4_PRODUCTION_COUNTERS width={WIDTH} control={control_samples:?} treatment={treatment_samples:?} invariant={diagonal_control}"
    );
}

#[test]
fn the_differential_judge_can_go_red() {
    let target = new_gint_par(7, Vec::new(), false);
    let mut recursive = RecursiveContext::new();
    let recursive_result = recursive
        .spatial_match(target.clone(), new_freevar_par(0, Vec::new()))
        .is_some();
    let mut pda = SpatialMatcherContext::new();
    let mutated_result = pda
        .spatial_match(target, new_gint_par(8, Vec::new(), false))
        .is_some();
    assert_ne!(recursive_result, mutated_result);
}

#[test]
fn oracle_provenance_is_test_only_and_distinct() {
    let recursive = include_str!("spatial_matcher_oracle/spatial_matcher.rs");
    let production = include_str!("../../src/rust/interpreter/matcher/spatial_matcher_pda.rs");
    assert!(recursive.contains("self.spatial_match("));
    assert!(production.contains("enum Job"));
    assert!(!production.contains("fn match_connective_with_bounds"));
    assert_ne!(recursive.as_bytes(), production.as_bytes());
}
