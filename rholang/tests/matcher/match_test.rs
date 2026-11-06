/*
 * See rholang/src/test/scala/coop/rchain/rholang/interpreter/matcher/MatchTest.scala
 *
 * It's important to note that in the Scala tests there are differences in the way they name their tests.
 * For example: 'should "work"', 'should "better be quick"', 'should "succeed"', etc...
 *
 * On the Scala side, there are multiple functions that create RhoTypes and implicitly pass them
 * to 'assertSpatialMatch' even though 'assertSpatialMatch' takes type 'Par'
 * For example: passing an 'Expr', 'Expr' to function that takes type 'Par', 'Par'
 *
 *
 * Might be able to use '::default()' at certain points
*/
use std::collections::BTreeMap;

use connective::ConnectiveInstance::*;
use expr::ExprInstance::*;
use g_unforgeable::UnfInstance::GPrivateBody;
use models::rhoapi::*;
use models::rust::par_map::ParMap;
use models::rust::par_map_type_mapper::ParMapTypeMapper;
use models::rust::par_set::ParSet;
use models::rust::par_set_type_mapper::ParSetTypeMapper;
use models::rust::rholang::implicits::vector_par;
use models::rust::utils::*;
use rholang::rust::interpreter::matcher::spatial_matcher::{SpatialMatcher, SpatialMatcherContext};
use rholang::rust::interpreter::util::{prepend_connective, prepend_expr};

fn assert_spatial_match(
    target: Par,
    pattern: Par,
    expected_captures: Option<FreeMap>,
) -> Result<(), String> {
    // println!("\ntest target: {:?}", target);
    // println!("\ntest pattern: {:?}", pattern);

    let mut spatial_matcher = SpatialMatcherContext::new();
    let spatial_match_result = spatial_matcher.spatial_match(target, pattern);
    let result = spatial_matcher.free_map;

    match (spatial_match_result.is_some(), expected_captures) {
        (true, Some(expected)) => assert_eq!(result, expected),
        (_, expected) => assert_eq!(None, expected),
    }

    Ok(())
}

#[test]
fn matching_ground_with_var_should_work() {
    let target = new_gint_par(7, Vec::new(), false);
    let pattern = new_freevar_par(0, Vec::new());

    let expected_captures = BTreeMap::from([(0, new_gint_par(7, Vec::new(), false))]);
    assert!(assert_spatial_match(target, pattern, Some(expected_captures)).is_ok());
}

#[test]
fn matching_bound_var_with_free_var_should_fail() {
    let target: Par = new_boundvar_par(0, Vec::new(), false);
    let pattern: Par = new_freevar_par(0, Vec::new());

    assert!(assert_spatial_match(target, pattern, None).is_ok());
}

#[test]
fn matching_bound_var_with_a_wildcard_should_succeed() {
    let target: Par = new_boundvar_par(0, Vec::new(), false);
    let pattern: Par = new_wildcard_par(Vec::new(), true);

    assert!(assert_spatial_match(target, pattern, Some(BTreeMap::new())).is_ok());
}

#[test]
fn matching_lists_of_grounds_with_lists_of_vars_should_work() {
    let target: Par = new_elist_par(
        vec![
            new_gstring_par("add".to_string(), Vec::new(), false),
            new_gint_par(7, Vec::new(), false),
            new_gint_par(8, Vec::new(), false),
        ],
        Vec::new(),
        false,
        None,
        Vec::new(),
        false,
    );

    let pattern: Par = new_elist_par(
        vec![
            new_gstring_par("add".to_string(), Vec::new(), false),
            new_freevar_par(0, Vec::new()),
            new_freevar_par(1, Vec::new()),
        ],
        Vec::new(),
        true,
        None,
        Vec::new(),
        true,
    );

    let expected_captures = BTreeMap::from([
        (0, new_gint_par(7, Vec::new(), false)),
        (1, new_gint_par(8, Vec::new(), false)),
    ]);
    assert!(assert_spatial_match(target, pattern, Some(expected_captures)).is_ok());
}

#[test]
fn matching_two_lists_in_parallel_should_work() {
    let target: Par = prepend_expr(
        new_elist_par(
            vec![
                new_gint_par(7, Vec::new(), false),
                new_gint_par(9, Vec::new(), false),
            ],
            Vec::new(),
            false,
            None,
            Vec::new(),
            false,
        ),
        new_elist_expr(
            vec![
                new_gint_par(7, Vec::new(), false),
                new_gint_par(8, Vec::new(), false),
            ],
            Vec::new(),
            false,
            None,
        ),
        0,
    );

    let pattern: Par = prepend_expr(
        new_elist_par(
            vec![
                new_freevar_par(0, Vec::new()),
                new_gint_par(9, Vec::new(), false),
            ],
            Vec::new(),
            true,
            None,
            Vec::new(),
            false,
        ),
        new_elist_expr(
            vec![
                new_gint_par(7, Vec::new(), false),
                new_freevar_par(1, Vec::new()),
            ],
            Vec::new(),
            true,
            None,
        ),
        1,
    );

    let expected_captures = BTreeMap::from([
        (0, new_gint_par(7, Vec::new(), false)),
        (1, new_gint_par(8, Vec::new(), false)),
    ]);
    assert!(assert_spatial_match(target, pattern, Some(expected_captures)).is_ok());
}

#[test]
fn matching_huge_list_of_targets_with_no_patterns_and_a_remainder_should_better_be_quick() {
    let target: Par = vector_par(Vec::new(), false).with_exprs(vec![new_gint_expr(1); 1000]);

    let pattern: Par = new_freevar_par(0, Vec::new());

    let expected_captures = BTreeMap::from([(0, target.clone())]);
    assert!(assert_spatial_match(target, pattern, Some(expected_captures)).is_ok());
}

#[test]
fn matching_a_sends_channel_should_work() {
    let target = new_send_par(
        vector_par(Vec::new(), false).with_unforgeables(vec![GUnforgeable {
            unf_instance: Some(GPrivateBody(GPrivate {
                id: "unforgeable".into(),
            })),
        }]),
        vec![
            new_gint_par(7, Vec::new(), false),
            new_gint_par(8, Vec::new(), false),
        ],
        false,
        Vec::new(),
        false,
        Vec::new(),
        false,
    );

    let pattern = new_send_par(
        new_freevar_par(0, Vec::new()),
        vec![
            new_wildcard_par(Vec::new(), true),
            new_gint_par(8, Vec::new(), false),
        ],
        false,
        Vec::new(),
        true,
        Vec::new(),
        true,
    );

    let expected_captures = BTreeMap::from([(
        0,
        vector_par(Vec::new(), false).with_unforgeables(vec![GUnforgeable {
            unf_instance: Some(GPrivateBody(GPrivate {
                id: "unforgeable".into(),
            })),
        }]),
    )]);
    assert!(assert_spatial_match(target, pattern, Some(expected_captures)).is_ok());
}

#[test]
fn matching_a_sends_body_should_work() {
    let target = new_send_par(
        vector_par(Vec::new(), false).with_unforgeables(vec![GUnforgeable {
            unf_instance: Some(GPrivateBody(GPrivate {
                id: "unforgeable".into(),
            })),
        }]),
        vec![
            new_gint_par(7, Vec::new(), false),
            new_gint_par(8, Vec::new(), false),
        ],
        false,
        Vec::new(),
        false,
        Vec::new(),
        false,
    );

    let pattern = new_send_par(
        new_wildcard_par(Vec::new(), true),
        vec![
            new_freevar_par(0, Vec::new()),
            new_gint_par(8, Vec::new(), false),
        ],
        false,
        Vec::new(),
        true,
        Vec::new(),
        true,
    );

    let expected_captures = BTreeMap::from([(0, new_gint_par(7, Vec::new(), false))]);
    assert!(assert_spatial_match(target, pattern, Some(expected_captures)).is_ok());
}

#[test]
fn matching_a_send_should_require_arity_matching() {
    let target = new_send_par(
        vector_par(Vec::new(), false).with_unforgeables(vec![GUnforgeable {
            unf_instance: Some(GPrivateBody(GPrivate {
                id: "unforgeable".into(),
            })),
        }]),
        vec![
            new_gint_par(7, Vec::new(), false),
            new_gint_par(8, Vec::new(), false),
            new_gint_par(9, Vec::new(), false),
        ],
        false,
        Vec::new(),
        false,
        Vec::new(),
        false,
    );

    let pattern = new_send_par(
        new_wildcard_par(Vec::new(), true),
        vec![
            new_freevar_par(0, Vec::new()),
            new_wildcard_par(Vec::new(), true),
        ],
        false,
        Vec::new(),
        true,
        Vec::new(),
        true,
    );

    assert!(assert_spatial_match(target, pattern, None).is_ok());
}

#[test]
fn matching_extras_with_free_variable_should_work() {
    let target: Par = prepend_expr(
        prepend_expr(new_gint_par(9, Vec::new(), false), new_gint_expr(8), 0),
        new_gint_expr(7),
        0,
    );

    let pattern: Par = prepend_expr(new_freevar_par(0, Vec::new()), new_gint_expr(8), 1);

    let expected_captures = BTreeMap::from([(
        0,
        prepend_expr(new_gint_par(9, Vec::new(), false), new_gint_expr(7), 0),
    )]);
    assert!(assert_spatial_match(target, pattern, Some(expected_captures)).is_ok());
}

#[test]
fn matching_a_singleton_list_should_work() {
    let target: Par = new_elist_par(
        vec![new_gint_par(1, Vec::new(), false)],
        Vec::new(),
        false,
        None,
        Vec::new(),
        false,
    );

    let pattern: Par = new_elist_par(
        vec![new_freevar_par(0, Vec::new())],
        Vec::new(),
        true,
        None,
        Vec::new(),
        true,
    );

    let expected_captures = BTreeMap::from([(0, new_gint_par(1, Vec::new(), false))]);
    assert!(assert_spatial_match(target, pattern, Some(expected_captures)).is_ok());
}

#[test]
fn matching_patterns_to_equal_targets_should_work() {
    let target: Par = prepend_expr(new_gint_par(1, Vec::new(), false), new_gint_expr(1), 0);
    let pattern: Par = prepend_expr(new_freevar_par(1, Vec::new()), new_freevar_expr(0), 0);

    let expected_captures = BTreeMap::from([(0, target.clone())]);
    assert!(assert_spatial_match(target, pattern, Some(expected_captures)).is_ok());
}

#[test]
fn matching_multiple_reminders_should_assign_captures_to_remainders_greedily() {
    let target: Par = prepend_expr(
        prepend_expr(new_gint_par(3, Vec::new(), false), new_gint_expr(2), 0),
        new_gint_expr(1),
        0,
    );

    let pattern: Par = prepend_expr(
        prepend_expr(new_wildcard_par(Vec::new(), false), new_freevar_expr(1), 0),
        new_freevar_expr(0),
        0,
    );

    let expected_captures = BTreeMap::from([(0, target.clone())]);
    assert!(assert_spatial_match(target, pattern, Some(expected_captures)).is_ok());
}

#[test]
fn matching_that_requires_revision_of_prior_matches_should_work() {
    let pattern = prepend_expr(
        new_elist_par(
            vec![
                new_freevar_par(1, Vec::new()),
                new_elist_par(
                    vec![new_gint_par(2, Vec::new(), false)],
                    Vec::new(),
                    false,
                    None,
                    Vec::new(),
                    false,
                ),
            ],
            Vec::new(),
            true,
            None,
            Vec::new(),
            true,
        ),
        new_elist_expr(
            vec![
                new_gint_par(1, Vec::new(), false),
                new_elist_par(
                    vec![new_freevar_par(0, Vec::new())],
                    Vec::new(),
                    true,
                    None,
                    Vec::new(),
                    true,
                ),
            ],
            Vec::new(),
            true,
            None,
        ),
        0,
    );

    let target: Par = prepend_expr(
        new_elist_par(
            vec![
                new_gint_par(1, Vec::new(), false),
                new_elist_par(
                    vec![new_gint_par(3, Vec::new(), false)],
                    Vec::new(),
                    false,
                    None,
                    Vec::new(),
                    false,
                ),
            ],
            Vec::new(),
            false,
            None,
            Vec::new(),
            false,
        ),
        new_elist_expr(
            vec![
                new_gint_par(1, Vec::new(), false),
                new_elist_par(
                    vec![new_gint_par(2, Vec::new(), false)],
                    Vec::new(),
                    false,
                    None,
                    Vec::new(),
                    false,
                ),
            ],
            Vec::new(),
            false,
            None,
        ),
        0,
    );

    let expected_captures = BTreeMap::from([
        (0, new_gint_par(3, Vec::new(), false)),
        (1, new_gint_par(1, Vec::new(), false)),
    ]);
    assert!(assert_spatial_match(target, pattern, Some(expected_captures)).is_ok());
}

#[test]
fn matching_that_requires_revision_of_prior_matches_should_work_with_remainders() {
    let pattern: Par = prepend_expr(
        new_elist_par(
            vec![
                new_freevar_par(1, Vec::new()),
                new_elist_par(
                    vec![new_gint_par(2, Vec::new(), false)],
                    Vec::new(),
                    false,
                    None,
                    Vec::new(),
                    false,
                ),
            ],
            Vec::new(),
            true,
            None,
            Vec::new(),
            true,
        ),
        new_elist_expr(
            vec![
                new_gint_par(1, Vec::new(), false),
                new_elist_par(
                    Vec::new(),
                    Vec::new(),
                    true,
                    Some(new_freevar_var(0)),
                    Vec::new(),
                    true,
                ),
            ],
            Vec::new(),
            true,
            None,
        ),
        0,
    );

    let target: Par = prepend_expr(
        new_elist_par(
            vec![
                new_gint_par(1, Vec::new(), false),
                new_elist_par(
                    vec![new_gint_par(3, Vec::new(), false)],
                    Vec::new(),
                    false,
                    None,
                    Vec::new(),
                    false,
                ),
            ],
            Vec::new(),
            false,
            None,
            Vec::new(),
            false,
        ),
        new_elist_expr(
            vec![
                new_gint_par(1, Vec::new(), false),
                new_elist_par(
                    vec![new_gint_par(2, Vec::new(), false)],
                    Vec::new(),
                    false,
                    None,
                    Vec::new(),
                    false,
                ),
            ],
            Vec::new(),
            false,
            None,
        ),
        0,
    );

    let expected_captures = BTreeMap::from([
        (
            0,
            new_elist_par(
                vec![new_gint_par(3, Vec::new(), false)],
                Vec::new(),
                false,
                None,
                Vec::new(),
                false,
            ),
        ),
        (1, new_gint_par(1, Vec::new(), false)),
    ]);
    assert!(assert_spatial_match(target, pattern, Some(expected_captures)).is_ok());
}

#[test]
fn matching_that_requires_revision_of_prior_matches_should_work_despite_having_other_terms_in_the_same_list(
) {
    let pattern: Par = prepend_expr(
        new_elist_par(
            vec![
                new_freevar_par(1, Vec::new()),
                new_elist_par(
                    vec![
                        new_gint_par(2, Vec::new(), false),
                        new_gint_par(2, Vec::new(), false),
                    ],
                    Vec::new(),
                    false,
                    None,
                    Vec::new(),
                    false,
                ),
            ],
            Vec::new(),
            true,
            None,
            Vec::new(),
            true,
        ),
        new_elist_expr(
            vec![
                new_gint_par(1, Vec::new(), false),
                new_elist_par(
                    vec![
                        new_freevar_par(0, Vec::new()),
                        new_gint_par(2, Vec::new(), false),
                    ],
                    Vec::new(),
                    true,
                    None,
                    Vec::new(),
                    true,
                ),
            ],
            Vec::new(),
            true,
            None,
        ),
        0,
    );

    let target: Par = prepend_expr(
        new_elist_par(
            vec![
                new_gint_par(1, Vec::new(), false),
                new_elist_par(
                    vec![
                        new_gint_par(3, Vec::new(), false),
                        new_gint_par(2, Vec::new(), false),
                    ],
                    Vec::new(),
                    false,
                    None,
                    Vec::new(),
                    false,
                ),
            ],
            Vec::new(),
            false,
            None,
            Vec::new(),
            false,
        ),
        new_elist_expr(
            vec![
                new_gint_par(1, Vec::new(), false),
                new_elist_par(
                    vec![
                        new_gint_par(2, Vec::new(), false),
                        new_gint_par(2, Vec::new(), false),
                    ],
                    Vec::new(),
                    false,
                    None,
                    Vec::new(),
                    false,
                ),
            ],
            Vec::new(),
            false,
            None,
        ),
        0,
    );

    let expected_captures = BTreeMap::from([
        (0, new_gint_par(3, Vec::new(), false)),
        (1, new_gint_par(1, Vec::new(), false)),
    ]);
    assert!(assert_spatial_match(target, pattern, Some(expected_captures)).is_ok());
}

#[test]
fn matching_that_requires_revision_of_prior_matches_should_work_with_remainders_and_wildcards() {
    let pattern: Par = prepend_expr(
        vector_par(Vec::new(), true).with_exprs(vec![new_freevar_expr(0)]),
        new_elist_expr(
            vec![new_wildcard_par(Vec::new(), true)],
            Vec::new(),
            true,
            None,
        ),
        0,
    );

    let target: Par = prepend_expr(
        vector_par(vec![0], false).with_exprs(vec![new_elist_expr(
            vec![new_boundvar_par(0, vec![0], false)],
            vec![0],
            false,
            None,
        )]),
        new_elist_expr(
            vec![new_gint_par(1, Vec::new(), false)],
            Vec::new(),
            false,
            None,
        ),
        0,
    );

    let expected_captures = BTreeMap::from([(
        0,
        new_elist_par(
            vec![new_gint_par(1, Vec::new(), false)],
            Vec::new(),
            false,
            None,
            Vec::new(),
            false,
        ),
    )]);
    assert!(assert_spatial_match(target, pattern, Some(expected_captures)).is_ok());
}

#[test]
fn matching_extras_with_wildcard_should_work() {
    let target: Par = prepend_expr(
        prepend_expr(
            vector_par(Vec::new(), false).with_exprs(vec![new_gint_expr(9)]),
            new_gint_expr(8),
            0,
        ),
        new_gint_expr(7),
        0,
    );

    let pattern: Par = prepend_expr(
        vector_par(Vec::new(), true).with_exprs(vec![new_wildcard_expr()]),
        new_gint_expr(8),
        1,
    );

    assert!(assert_spatial_match(target, pattern, Some(BTreeMap::new())).is_ok());
}

#[test]
fn matching_extras_with_wildcard_and_free_variable_should_capture_in_the_free_variable() {
    let target: Par = prepend_expr(
        prepend_expr(
            vector_par(Vec::new(), false).with_exprs(vec![new_gint_expr(9)]),
            new_gint_expr(8),
            0,
        ),
        new_gint_expr(7),
        0,
    );

    let pattern: Par = prepend_expr(
        prepend_expr(
            vector_par(Vec::new(), true).with_exprs(vec![new_wildcard_expr()]),
            new_freevar_expr(0),
            1,
        ),
        new_gint_expr(8),
        1,
    );

    let expected_captures = BTreeMap::from([(
        0,
        prepend_expr(new_gint_par(9, Vec::new(), false), new_gint_expr(7), 0),
    )]);
    assert!(assert_spatial_match(target, pattern, Some(expected_captures)).is_ok());
}

#[test]
fn matching_send_with_free_variable_in_channel_and_variable_position_should_capture_both_values() {
    let target: Par = new_send_par(
        vector_par(Vec::new(), false).with_unforgeables(vec![GUnforgeable {
            unf_instance: Some(GPrivateBody(GPrivate { id: "zero".into() })),
        }]),
        vec![
            new_gint_par(7, Vec::new(), false),
            vector_par(Vec::new(), false).with_unforgeables(vec![GUnforgeable {
                unf_instance: Some(GPrivateBody(GPrivate { id: "one".into() })),
            }]),
        ],
        false,
        Vec::new(),
        false,
        Vec::new(),
        false,
    );

    let pattern: Par = new_send_par(
        vector_par(Vec::new(), true).with_exprs(vec![new_freevar_expr(0)]),
        vec![
            new_gint_par(7, Vec::new(), false),
            new_freevar_par(1, Vec::new()),
        ],
        false,
        Vec::new(),
        true,
        Vec::new(),
        true,
    );

    let expected_captures = BTreeMap::from([
        (
            0,
            vector_par(Vec::new(), false).with_unforgeables(vec![GUnforgeable {
                unf_instance: Some(GPrivateBody(GPrivate { id: "zero".into() })),
            }]),
        ),
        (
            1,
            vector_par(Vec::new(), false).with_unforgeables(vec![GUnforgeable {
                unf_instance: Some(GPrivateBody(GPrivate { id: "one".into() })),
            }]),
        ),
    ]);
    assert!(assert_spatial_match(target, pattern, Some(expected_captures)).is_ok());
}

#[test]
fn matching_a_receive_with_a_free_variable_in_the_channel_and_a_free_variable_in_the_body_should_capture_for_both_variables(
) {
    let target: Par = new_receive_par(
        vec![
            ReceiveBind {
                patterns: vec![
                    new_freevar_par(0, Vec::new()),
                    new_freevar_par(1, Vec::new()),
                ],
                source: Some(new_gint_par(7, Vec::new(), false)),
                remainder: None,
                free_count: 0,
            },
            ReceiveBind {
                patterns: vec![
                    new_freevar_par(0, Vec::new()),
                    new_freevar_par(1, Vec::new()),
                ],
                source: Some(new_gint_par(8, Vec::new(), false)),
                remainder: None,
                free_count: 0,
            },
        ],
        new_send_par(
            vector_par(Vec::new(), false).with_unforgeables(vec![GUnforgeable {
                unf_instance: Some(GPrivateBody(GPrivate {
                    id: "unforgeable".into(),
                })),
            }]),
            vec![
                new_gint_par(9, Vec::new(), false),
                new_gint_par(10, Vec::new(), false),
            ],
            false,
            Vec::new(),
            false,
            Vec::new(),
            false,
        ),
        false,
        false,
        4,
        Vec::new(),
        false,
        Vec::new(),
        false,
    );

    let pattern: Par = new_receive_par(
        vec![
            ReceiveBind {
                patterns: vec![
                    new_freevar_par(0, Vec::new()),
                    new_freevar_par(1, Vec::new()),
                ],
                source: Some(new_gint_par(7, Vec::new(), false)),
                remainder: None,
                free_count: 0,
            },
            ReceiveBind {
                patterns: vec![
                    new_freevar_par(0, Vec::new()),
                    new_freevar_par(1, Vec::new()),
                ],
                source: Some(new_freevar_par(0, Vec::new())),
                remainder: None,
                free_count: 0,
            },
        ],
        new_freevar_par(1, Vec::new()),
        false,
        false,
        4,
        Vec::new(),
        true,
        Vec::new(),
        true,
    );

    let expected_captures = BTreeMap::from([
        (0, new_gint_par(8, Vec::new(), false)),
        (
            1,
            new_send_par(
                vector_par(Vec::new(), false).with_unforgeables(vec![GUnforgeable {
                    unf_instance: Some(GPrivateBody(GPrivate {
                        id: "unforgeable".into(),
                    })),
                }]),
                vec![
                    new_gint_par(9, Vec::new(), false),
                    new_gint_par(10, Vec::new(), false),
                ],
                false,
                Vec::new(),
                false,
                Vec::new(),
                false,
            ),
        ),
    ]);
    assert!(assert_spatial_match(target, pattern.clone(), Some(expected_captures)).is_ok());
}

#[test]
fn matching_an_eval_with_no_free_variables_should_succeed_but_not_capture_anything() {
    let target: Par = new_boundvar_par(0, vec![0], false);
    let pattern: Par = new_boundvar_par(0, vec![0], false);

    assert!(assert_spatial_match(target, pattern.clone(), Some(BTreeMap::new())).is_ok());
}

#[test]
fn matching_between_news_should_match_the_bodies_if_the_new_count_is_the_same() {
    let target: Par = new_new_par(
        2,
        vector_par(Vec::new(), false)
            .prepend_send(new_send(
                new_boundvar_par(0, vec![0], false),
                vec![new_gint_par(43, Vec::new(), false)],
                false,
                vec![0],
                false,
            ))
            .prepend_send(new_send(
                new_gint_par(7, Vec::new(), false),
                vec![new_gint_par(42, Vec::new(), false)],
                false,
                Vec::new(),
                false,
            )),
        Vec::new(),
        BTreeMap::new(),
        Vec::new(),
        Vec::new(),
        false,
    );

    let pattern: Par = new_new_par(
        2,
        prepend_expr(
            vector_par(Vec::new(), false).prepend_send(new_send(
                new_gint_par(7, Vec::new(), false),
                vec![new_freevar_par(0, Vec::new())],
                false,
                Vec::new(),
                true,
            )),
            new_wildcard_expr(),
            1,
        ),
        Vec::new(),
        BTreeMap::new(),
        Vec::new(),
        Vec::new(),
        true,
    );

    let expected_captures = BTreeMap::from([(0, new_gint_par(42, Vec::new(), false))]);
    assert!(assert_spatial_match(target, pattern.clone(), Some(expected_captures)).is_ok());
}

#[test]
fn matching_between_matches_should_require_equality_of_cases_but_match_targets_and_inside_bodies() {
    let target: Par = new_match_par(
        new_elist_par(
            vec![
                new_gint_par(4, Vec::new(), false),
                new_gint_par(20, Vec::new(), false),
            ],
            Vec::new(),
            false,
            None,
            Vec::new(),
            false,
        ),
        vec![
            MatchCase {
                pattern: Some(new_elist_par(
                    vec![
                        new_freevar_par(0, Vec::new()),
                        new_freevar_par(1, Vec::new()),
                    ],
                    Vec::new(),
                    false,
                    None,
                    Vec::new(),
                    false,
                )),
                source: Some(new_send_par(
                    new_boundvar_par(1, vec![1], false),
                    vec![new_boundvar_par(0, vec![0], false)],
                    false,
                    vec![0, 1],
                    false,
                    vec![0, 1],
                    false,
                )),
                free_count: 0,
                guard: None,
            },
            MatchCase {
                pattern: Some(new_wildcard_par(Vec::new(), true)),
                source: Some(vector_par(Vec::new(), false)),
                free_count: 0,
                guard: None,
            },
        ],
        Vec::new(),
        false,
        Vec::new(),
        false,
    );

    let pattern: Par = new_match_par(
        new_freevar_par(0, Vec::new()),
        vec![
            MatchCase {
                pattern: Some(new_elist_par(
                    vec![
                        new_freevar_par(0, Vec::new()),
                        new_freevar_par(1, Vec::new()),
                    ],
                    Vec::new(),
                    false,
                    None,
                    Vec::new(),
                    false,
                )),
                source: Some(new_wildcard_par(Vec::new(), true)),
                free_count: 0,
                guard: None,
            },
            MatchCase {
                pattern: Some(new_wildcard_par(Vec::new(), true)),
                source: Some(new_freevar_par(1, Vec::new())),
                free_count: 0,
                guard: None,
            },
        ],
        Vec::new(),
        true,
        Vec::new(),
        true,
    );

    let expected_captures = BTreeMap::from([
        (
            0,
            new_elist_par(
                vec![
                    new_gint_par(4, Vec::new(), false),
                    new_gint_par(20, Vec::new(), false),
                ],
                Vec::new(),
                false,
                None,
                Vec::new(),
                false,
            ),
        ),
        (1, Par::default()),
    ]);
    assert!(assert_spatial_match(target, pattern.clone(), Some(expected_captures)).is_ok());
}

#[test]
fn matching_a_list_with_a_remainder_should_capture_the_remainder() {
    let target: Par = new_elist_par(
        vec![
            new_gint_par(1, Vec::new(), false),
            new_gint_par(2, Vec::new(), false),
        ],
        Vec::new(),
        false,
        None,
        Vec::new(),
        false,
    );

    let pattern: Par = new_elist_par(
        vec![new_gint_par(1, Vec::new(), false)],
        Vec::new(),
        true,
        Some(new_freevar_var(0)),
        Vec::new(),
        true,
    );

    let expected_captures = BTreeMap::from([(
        0,
        new_elist_par(
            vec![new_gint_par(2, Vec::new(), false)],
            Vec::new(),
            false,
            None,
            Vec::new(),
            false,
        ),
    )]);
    assert!(assert_spatial_match(target, pattern.clone(), Some(expected_captures)).is_ok());
}

#[test]
fn matching_sets_should_work_for_concrete_sets() {
    let target: Par = new_eset_par(
        vec![
            new_gint_par(1, Vec::new(), false),
            new_gint_par(2, Vec::new(), false),
            new_gint_par(3, Vec::new(), false),
        ],
        Vec::new(),
        false,
        None,
        Vec::new(),
        false,
    );

    let pattern: Par = target.clone();

    assert!(assert_spatial_match(target, pattern.clone(), Some(BTreeMap::new())).is_ok());
}

#[test]
fn matching_sets_should_work_with_free_variables_and_wildcards() {
    let target: Par = new_eset_par(
        vec![
            new_gint_par(1, Vec::new(), false),
            new_gint_par(2, Vec::new(), false),
            new_gint_par(3, Vec::new(), false),
            new_gint_par(4, Vec::new(), false),
            new_gint_par(5, Vec::new(), false),
        ],
        Vec::new(),
        false,
        None,
        Vec::new(),
        false,
    );

    let pattern: Par = new_eset_par(
        vec![
            new_gint_par(2, Vec::new(), false),
            new_gint_par(5, Vec::new(), false),
            new_freevar_par(0, Vec::new()),
            new_freevar_par(1, Vec::new()),
            new_wildcard_par(Vec::new(), true),
        ],
        Vec::new(),
        true,
        None,
        Vec::new(),
        true,
    );

    let expected_captures = BTreeMap::from([
        (0, new_gint_par(4, Vec::new(), false)),
        (1, new_gint_par(3, Vec::new(), false)),
    ]);
    assert!(assert_spatial_match(target.clone(), pattern, Some(expected_captures)).is_ok());

    let no_matching_pattern = new_eset_par(
        vec![
            new_gint_par(2, Vec::new(), false),
            new_gint_par(5, Vec::new(), false),
            new_freevar_par(0, Vec::new()),
            new_wildcard_par(Vec::new(), true),
        ],
        Vec::new(),
        true,
        None,
        Vec::new(),
        true,
    );

    assert!(assert_spatial_match(target, no_matching_pattern, None).is_ok());
}

#[test]
fn matching_sets_should_work_with_wildcard_remainders() {
    let target_elements = vec![
        new_gint_par(1, Vec::new(), false),
        new_gint_par(2, Vec::new(), false),
        new_gint_par(3, Vec::new(), false),
        new_gint_par(4, Vec::new(), false),
        new_gint_par(5, Vec::new(), false),
    ];

    let target: Par = new_eset_par(
        target_elements.clone(),
        Vec::new(),
        false,
        None,
        Vec::new(),
        false,
    );
    let pattern: Par = new_eset_par(
        vec![
            new_gint_par(1, Vec::new(), false),
            new_gint_par(4, Vec::new(), false),
        ],
        Vec::new(),
        true,
        Some(new_wildcard_var()),
        Vec::new(),
        true,
    );

    assert!(assert_spatial_match(target.clone(), pattern.clone(), Some(BTreeMap::new())).is_ok());

    let all_elements_and_wildcard: Par = new_eset_par(
        target_elements,
        Vec::new(),
        true,
        Some(new_wildcard_var()),
        Vec::new(),
        true,
    );

    assert!(assert_spatial_match(
        target.clone(),
        all_elements_and_wildcard,
        Some(BTreeMap::new())
    )
    .is_ok());

    let just_wildcard = new_eset_par(
        Vec::new(),
        Vec::new(),
        true,
        Some(new_wildcard_var()),
        Vec::new(),
        true,
    );

    assert!(assert_spatial_match(target, just_wildcard, Some(BTreeMap::new())).is_ok());
}

#[test]
fn matching_sets_should_work_with_var_remainders() {
    let target_elements = vec![
        new_gint_par(1, Vec::new(), false),
        new_gint_par(2, Vec::new(), false),
        new_gint_par(3, Vec::new(), false),
        new_gint_par(4, Vec::new(), false),
        new_gint_par(5, Vec::new(), false),
    ];

    let target: Par = new_eset_par(
        target_elements.clone(),
        Vec::new(),
        false,
        None,
        Vec::new(),
        false,
    );
    let pattern: Par = new_eset_par(
        vec![
            new_gint_par(1, Vec::new(), false),
            new_gint_par(4, Vec::new(), false),
            new_freevar_par(0, Vec::new()),
        ],
        Vec::new(),
        true,
        Some(new_freevar_var(1)),
        Vec::new(),
        true,
    );

    let expected_captures = BTreeMap::from([
        (0, new_gint_par(2, Vec::new(), false)),
        (
            1,
            new_eset_par(
                vec![
                    new_gint_par(3, Vec::new(), false),
                    new_gint_par(5, Vec::new(), false),
                ],
                Vec::new(),
                false,
                None,
                Vec::new(),
                false,
            ),
        ),
    ]);
    assert!(assert_spatial_match(target.clone(), pattern, Some(expected_captures)).is_ok());

    let all_elements_and_remainder: Par = new_eset_par(
        target_elements.clone(),
        Vec::new(),
        true,
        Some(new_freevar_var(0)),
        Vec::new(),
        true,
    );

    assert!(assert_spatial_match(
        target.clone(),
        all_elements_and_remainder,
        Some(BTreeMap::from([(
            0,
            new_eset_par(vec![], Vec::new(), false, None, Vec::new(), false)
        )]))
    )
    .is_ok());

    let just_remainder = new_eset_par(
        Vec::new(),
        Vec::new(),
        true,
        Some(new_freevar_var(0)),
        Vec::new(),
        true,
    );

    assert!(assert_spatial_match(
        target,
        just_remainder,
        Some(BTreeMap::from([(
            0,
            new_eset_par(target_elements, Vec::new(), false, None, Vec::new(), false)
        )]))
    )
    .is_ok());
}

#[test]
fn matching_maps_should_work_for_concrete_maps() {
    let target: Par = new_emap_par(
        vec![
            new_key_value_pair(
                new_gint_par(1, Vec::new(), false),
                new_gint_par(2, Vec::new(), false),
            ),
            new_key_value_pair(
                new_gint_par(3, Vec::new(), false),
                new_gint_par(4, Vec::new(), false),
            ),
        ],
        Vec::new(),
        false,
        None,
        Vec::new(),
        false,
    );

    let pattern: Par = target.clone();
    assert!(assert_spatial_match(target, pattern, Some(BTreeMap::new())).is_ok());
}

#[test]
fn matching_maps_should_work_with_free_variables_and_wildcards() {
    let target: Par = new_emap_par(
        vec![
            new_key_value_pair(
                new_gint_par(1, Vec::new(), false),
                new_gint_par(2, Vec::new(), false),
            ),
            new_key_value_pair(
                new_gint_par(3, Vec::new(), false),
                new_gint_par(4, Vec::new(), false),
            ),
            new_key_value_pair(
                new_gint_par(5, Vec::new(), false),
                new_gint_par(6, Vec::new(), false),
            ),
        ],
        Vec::new(),
        false,
        None,
        Vec::new(),
        false,
    );

    let pattern: Par = new_emap_par(
        vec![
            new_key_value_pair(
                new_gint_par(1, Vec::new(), false),
                new_freevar_par(1, Vec::new()),
            ),
            new_key_value_pair(
                new_gint_par(3, Vec::new(), false),
                new_gint_par(4, Vec::new(), false),
            ),
            new_key_value_pair(
                new_freevar_par(0, Vec::new()),
                new_wildcard_par(Vec::new(), true),
            ),
        ],
        vec![0, 1],
        true,
        None,
        vec![0, 1],
        true,
    );

    let expected_captures = BTreeMap::from([
        (0, new_gint_par(5, Vec::new(), false)),
        (1, new_gint_par(2, Vec::new(), false)),
    ]);
    assert!(assert_spatial_match(target.clone(), pattern, Some(expected_captures)).is_ok());

    let non_matching_pattern = new_emap_par(
        vec![
            new_key_value_pair(
                new_gint_par(3, Vec::new(), false),
                new_freevar_par(1, Vec::new()),
            ),
            new_key_value_pair(
                new_freevar_par(0, Vec::new()),
                new_wildcard_par(Vec::new(), true),
            ),
            new_key_value_pair(
                new_wildcard_par(Vec::new(), true),
                new_gint_par(4, Vec::new(), false),
            ),
        ],
        vec![0, 1],
        true,
        None,
        vec![0, 1],
        true,
    );

    assert!(assert_spatial_match(target, non_matching_pattern, None).is_ok());
}

#[test]
fn matching_maps_should_work_with_wildcard_remainders() {
    let target: Par = new_emap_par(
        vec![
            new_key_value_pair(
                new_gint_par(1, Vec::new(), false),
                new_gint_par(2, Vec::new(), false),
            ),
            new_key_value_pair(
                new_gint_par(3, Vec::new(), false),
                new_gint_par(4, Vec::new(), false),
            ),
            new_key_value_pair(
                new_gint_par(5, Vec::new(), false),
                new_gint_par(6, Vec::new(), false),
            ),
        ],
        Vec::new(),
        false,
        None,
        Vec::new(),
        false,
    );

    let pattern: Par = new_emap_par(
        vec![new_key_value_pair(
            new_gint_par(3, Vec::new(), false),
            new_gint_par(4, Vec::new(), false),
        )],
        vec![],
        true,
        Some(new_wildcard_var()),
        vec![],
        true,
    );

    assert!(assert_spatial_match(target.clone(), pattern, Some(BTreeMap::new())).is_ok());

    let all_elements_and_wildcard = new_emap_par(
        vec![
            new_key_value_pair(
                new_gint_par(1, Vec::new(), false),
                new_gint_par(2, Vec::new(), false),
            ),
            new_key_value_pair(
                new_gint_par(3, Vec::new(), false),
                new_gint_par(4, Vec::new(), false),
            ),
            new_key_value_pair(
                new_gint_par(5, Vec::new(), false),
                new_gint_par(6, Vec::new(), false),
            ),
        ],
        vec![],
        true,
        Some(new_wildcard_var()),
        vec![],
        true,
    );

    assert!(assert_spatial_match(
        target.clone(),
        all_elements_and_wildcard,
        Some(BTreeMap::new())
    )
    .is_ok());

    let just_wildcard = new_emap_par(vec![], vec![], true, Some(new_wildcard_var()), vec![], true);

    assert!(assert_spatial_match(target.clone(), just_wildcard, Some(BTreeMap::new())).is_ok());
}

#[test]
fn matching_maps_should_work_with_var_remainders() {
    let target_elements = vec![
        new_key_value_pair(
            new_gint_par(1, Vec::new(), false),
            new_gint_par(2, Vec::new(), false),
        ),
        new_key_value_pair(
            new_gint_par(3, Vec::new(), false),
            new_gint_par(4, Vec::new(), false),
        ),
        new_key_value_pair(
            new_gint_par(5, Vec::new(), false),
            new_gint_par(6, Vec::new(), false),
        ),
    ];
    let target: Par = new_emap_par(
        target_elements.clone(),
        Vec::new(),
        false,
        None,
        Vec::new(),
        false,
    );

    let pattern: Par = new_emap_par(
        vec![new_key_value_pair(
            new_freevar_par(0, Vec::new()),
            new_gint_par(4, Vec::new(), false),
        )],
        vec![],
        true,
        Some(new_freevar_var(1)),
        vec![],
        true,
    );

    let expected_captures = BTreeMap::from([
        (0, new_gint_par(3, Vec::new(), false)),
        (
            1,
            new_emap_par(
                vec![
                    new_key_value_pair(
                        new_gint_par(1, Vec::new(), false),
                        new_gint_par(2, Vec::new(), false),
                    ),
                    new_key_value_pair(
                        new_gint_par(5, Vec::new(), false),
                        new_gint_par(6, Vec::new(), false),
                    ),
                ],
                Vec::new(),
                false,
                None,
                Vec::new(),
                false,
            ),
        ),
    ]);

    assert!(assert_spatial_match(target.clone(), pattern.clone(), Some(expected_captures)).is_ok());

    let all_elements_and_remainder = new_emap_par(
        target_elements.clone(),
        vec![],
        true,
        Some(new_freevar_var(0)),
        vec![],
        true,
    );

    let expected_captures = BTreeMap::from([(
        0,
        new_emap_par(vec![], Vec::new(), false, None, Vec::new(), false),
    )]);
    assert!(assert_spatial_match(
        target.clone(),
        all_elements_and_remainder,
        Some(expected_captures)
    )
    .is_ok());

    let just_remainder = new_emap_par(vec![], vec![], true, Some(new_freevar_var(0)), vec![], true);

    let expected_captures = BTreeMap::from([(
        0,
        new_emap_par(target_elements, Vec::new(), false, None, Vec::new(), false),
    )]);
    assert!(assert_spatial_match(target.clone(), just_remainder, Some(expected_captures)).is_ok());
}

#[test]
fn matching_a_whole_list_with_a_remainder_should_capture_the_list() {
    let target: Par = new_elist_par(
        vec![
            new_gint_par(1, Vec::new(), false),
            new_gint_par(2, Vec::new(), false),
            new_gint_par(3, Vec::new(), false),
        ],
        Vec::new(),
        false,
        None,
        Vec::new(),
        false,
    );

    let pattern = new_elist_par(
        vec![],
        Vec::new(),
        true,
        Some(new_freevar_var(0)),
        Vec::new(),
        true,
    );

    let expected_captures = BTreeMap::from([(0, target.clone())]);
    assert!(assert_spatial_match(target, pattern.clone(), Some(expected_captures)).is_ok());
}

#[test]
fn matching_inside_bundles_should_not_be_possible() {
    let target: Par = vector_par(Vec::new(), false).with_bundles(vec![Bundle {
        body: Some(
            vector_par(Vec::new(), false)
                .prepend_send(new_send(
                    vector_par(Vec::new(), false).with_unforgeables(vec![GUnforgeable {
                        unf_instance: Some(GPrivateBody(GPrivate { id: "0".into() })),
                    }]),
                    vec![new_gint_par(43, Vec::new(), false)],
                    false,
                    Vec::new(),
                    false,
                ))
                .prepend_send(new_send(
                    new_gint_par(7, Vec::new(), false),
                    vec![new_gint_par(42, Vec::new(), false)],
                    false,
                    Vec::new(),
                    false,
                )),
        ),
        write_flag: false,
        read_flag: false,
    }]);

    let pattern: Par = vector_par(Vec::new(), false).with_bundles(vec![Bundle {
        body: Some(prepend_expr(
            vector_par(Vec::new(), false).prepend_send(new_send(
                new_gint_par(7, Vec::new(), false),
                vec![new_freevar_par(0, Vec::new())],
                false,
                Vec::new(),
                false,
            )),
            new_wildcard_expr(),
            1,
        )),
        write_flag: false,
        read_flag: false,
    }]);

    assert!(assert_spatial_match(target, pattern.clone(), None).is_ok());
}

#[test]
fn matching_inside_bundles_should_be_possible_to_match_on_entire_bundle() {
    let target: Par = vector_par(Vec::new(), false).with_bundles(vec![Bundle {
        body: Some(
            vector_par(Vec::new(), false)
                .prepend_send(new_send(
                    vector_par(Vec::new(), false).with_unforgeables(vec![GUnforgeable {
                        unf_instance: Some(GPrivateBody(GPrivate { id: "0".into() })),
                    }]),
                    vec![new_gint_par(43, Vec::new(), false)],
                    false,
                    Vec::new(),
                    false,
                ))
                .prepend_send(new_send(
                    new_gint_par(7, Vec::new(), false),
                    vec![new_gint_par(42, Vec::new(), false)],
                    false,
                    Vec::new(),
                    false,
                )),
        ),
        write_flag: false,
        read_flag: false,
    }]);

    let pattern: Par = new_freevar_par(0, Vec::new());

    let expected_captures = BTreeMap::from([(0, target.clone())]);
    assert!(assert_spatial_match(target, pattern.clone(), Some(expected_captures)).is_ok());
}

#[test]
fn matching_a_single_and_should_match_both_sides() {
    let target: Par = new_send_par(
        new_gint_par(7, Vec::new(), false),
        vec![new_gint_par(8, Vec::new(), false)],
        false,
        Vec::new(),
        false,
        Vec::new(),
        false,
    );

    let fail_target = new_send_par(
        new_gint_par(7, Vec::new(), false),
        vec![new_gint_par(9, Vec::new(), false)],
        false,
        Vec::new(),
        false,
        Vec::new(),
        false,
    );

    let pattern: Par = new_conn_and_body_par(
        vec![
            new_send_par(
                new_gint_par(7, Vec::new(), false),
                vec![new_freevar_par(0, Vec::new())],
                false,
                Vec::new(),
                true,
                Vec::new(),
                true,
            ),
            new_send_par(
                new_freevar_par(1, Vec::new()),
                vec![new_gint_par(8, Vec::new(), false)],
                false,
                Vec::new(),
                true,
                Vec::new(),
                true,
            ),
        ],
        Vec::new(),
        true,
    );

    let expected_captures = BTreeMap::from([
        (0, new_gint_par(8, Vec::new(), false)),
        (1, new_gint_par(7, Vec::new(), false)),
    ]);
    assert!(assert_spatial_match(target, pattern.clone(), Some(expected_captures)).is_ok());
    assert!(assert_spatial_match(fail_target, pattern, None).is_ok());
}

#[test]
fn matching_a_single_or_should_match_some_side() {
    let target: Par = new_send_par(
        new_gint_par(7, Vec::new(), false),
        vec![new_gint_par(8, Vec::new(), false)],
        false,
        Vec::new(),
        false,
        Vec::new(),
        false,
    );

    let fail_target: Par = new_send_par(
        new_gint_par(7, Vec::new(), false),
        vec![new_gint_par(9, Vec::new(), false)],
        false,
        Vec::new(),
        false,
        Vec::new(),
        false,
    );

    let pattern: Par = new_conn_or_body_par(
        vec![
            new_send_par(
                new_gint_par(9, Vec::new(), false),
                vec![new_freevar_par(0, Vec::new())],
                false,
                Vec::new(),
                true,
                Vec::new(),
                true,
            ),
            new_send_par(
                new_freevar_par(0, Vec::new()),
                vec![new_gint_par(8, Vec::new(), false)],
                false,
                Vec::new(),
                true,
                Vec::new(),
                true,
            ),
        ],
        Vec::new(),
        true,
    );

    assert!(assert_spatial_match(target, pattern.clone(), Some(BTreeMap::new())).is_ok());
    assert!(assert_spatial_match(fail_target, pattern, None).is_ok());
}

#[test]
fn matching_negation_should_work() {
    let target: Par = vector_par(Vec::new(), false).with_sends(vec![
        new_send(
            new_gint_par(1, Vec::new(), false),
            vec![new_gint_par(2, Vec::new(), false)],
            false,
            Vec::new(),
            false,
        ),
        new_send(
            new_gint_par(2, Vec::new(), false),
            vec![new_gint_par(3, Vec::new(), false)],
            false,
            Vec::new(),
            false,
        ),
        new_send(
            new_gint_par(3, Vec::new(), false),
            vec![new_gint_par(4, Vec::new(), false)],
            false,
            Vec::new(),
            false,
        ),
    ]);

    let pattern_connective: Connective = Connective {
        connective_instance: Some(ConnNotBody(vector_par(Vec::new(), false))),
    };

    let pattern: Par =
        vector_par(Vec::new(), true).with_connectives(vec![pattern_connective.clone()]);

    assert!(assert_spatial_match(target.clone(), pattern.clone(), Some(BTreeMap::new())).is_ok());

    let double_pattern: Par = pattern
        .clone()
        .with_connectives(vec![pattern_connective.clone(), pattern_connective.clone()]);

    assert!(assert_spatial_match(target.clone(), double_pattern, Some(BTreeMap::new())).is_ok());

    let triple_pattern = pattern.clone().with_connectives(vec![
        pattern_connective.clone(),
        pattern_connective.clone(),
        pattern_connective.clone(),
    ]);

    assert!(assert_spatial_match(target.clone(), triple_pattern, Some(BTreeMap::new())).is_ok());

    let quadruple_pattern = pattern.clone().with_connectives(vec![
        pattern_connective.clone(),
        pattern_connective.clone(),
        pattern_connective.clone(),
        pattern_connective.clone(),
    ]);

    assert!(assert_spatial_match(target, quadruple_pattern, None).is_ok());
}

#[test]
fn matching_a_complicated_connective_should_work() {
    let target: Par = vector_par(Vec::new(), false).with_sends(vec![
        new_send(
            new_gint_par(1, Vec::new(), false),
            vec![new_gint_par(6, Vec::new(), false)],
            false,
            Vec::new(),
            false,
        ),
        new_send(
            new_gint_par(2, Vec::new(), false),
            vec![new_gint_par(7, Vec::new(), false)],
            false,
            Vec::new(),
            false,
        ),
        new_send(
            new_gint_par(3, Vec::new(), false),
            vec![new_gint_par(8, Vec::new(), false)],
            false,
            Vec::new(),
            false,
        ),
    ]);

    let fail_target: Par = vector_par(Vec::new(), false).with_sends(vec![
        new_send(
            new_gint_par(1, Vec::new(), false),
            vec![new_gint_par(6, Vec::new(), false)],
            false,
            Vec::new(),
            false,
        ),
        new_send(
            new_gint_par(2, Vec::new(), false),
            vec![new_gint_par(9, Vec::new(), false)],
            false,
            Vec::new(),
            false,
        ),
        new_send(
            new_gint_par(3, Vec::new(), false),
            vec![new_gint_par(8, Vec::new(), false)],
            false,
            Vec::new(),
            false,
        ),
    ]);

    let non_null_connective: Connective = Connective {
        connective_instance: Some(ConnNotBody(vector_par(Vec::new(), false))),
    };

    let non_null_par: Par =
        vector_par(Vec::new(), true).with_connectives(vec![non_null_connective]);

    let single_factor: Connective = Connective {
        connective_instance: Some(ConnNotBody(vector_par(Vec::new(), true).with_connectives(
            vec![
                Connective {
                    connective_instance: Some(ConnNotBody(vector_par(Vec::new(), false))),
                },
                Connective {
                    connective_instance: Some(ConnNotBody(vector_par(Vec::new(), false))),
                },
            ],
        ))),
    };

    let single_factor_par: Par = vector_par(Vec::new(), true).with_connectives(vec![single_factor]);

    let capture: Connective = Connective {
        connective_instance: Some(ConnAndBody(ConnectiveBody {
            ps: vec![
                new_freevar_par(0, Vec::new()),
                new_send_par(
                    new_freevar_par(1, Vec::new()),
                    vec![new_gint_par(7, Vec::new(), false)],
                    false,
                    Vec::new(),
                    true,
                    Vec::new(),
                    true,
                ),
            ],
        })),
    };

    let prime: Connective = Connective {
        connective_instance: Some(ConnAndBody(ConnectiveBody {
            ps: vec![non_null_par.clone(), single_factor_par.clone()],
        })),
    };

    let alternative: Connective = Connective {
        connective_instance: Some(ConnOrBody(ConnectiveBody {
            ps: vec![
                new_send_par(
                    new_freevar_par(0, Vec::new()),
                    vec![new_gint_par(7, Vec::new(), false)],
                    false,
                    Vec::new(),
                    true,
                    Vec::new(),
                    true,
                ),
                new_send_par(
                    new_freevar_par(0, Vec::new()),
                    vec![new_gint_par(8, Vec::new(), false)],
                    false,
                    Vec::new(),
                    true,
                    Vec::new(),
                    true,
                ),
            ],
        })),
    };

    let pattern = vector_par(Vec::new(), true).with_connectives(vec![capture, prime, alternative]);

    let expected_captures = BTreeMap::from([
        (
            0,
            new_send_par(
                new_gint_par(2, Vec::new(), false),
                vec![new_gint_par(7, Vec::new(), false)],
                false,
                Vec::new(),
                false,
                Vec::new(),
                false,
            ),
        ),
        (1, new_gint_par(2, Vec::new(), false)),
    ]);
    assert!(assert_spatial_match(target, pattern.clone(), Some(expected_captures)).is_ok());
    assert!(assert_spatial_match(fail_target, pattern, None).is_ok());
}

#[test]
fn matching_a_target_with_var_ref_and_a_pattern_with_a_var_ref_should_ignore_locally_free() {
    let target: Par = new_new_par(
        1,
        new_receive_par(
            vec![ReceiveBind {
                patterns: vec![prepend_connective(
                    vector_par(Vec::new(), false),
                    Connective {
                        connective_instance: Some(VarRefBody(VarRef { index: 0, depth: 1 })),
                    },
                    1,
                )],
                source: Some(vector_par(Vec::new(), false)),
                remainder: None,
                free_count: 0,
            }],
            vector_par(Vec::new(), false),
            false,
            false,
            0,
            vec![0],
            false,
            vec![0],
            false,
        ),
        Vec::new(),
        BTreeMap::new(),
        Vec::new(),
        Vec::new(),
        false,
    );

    let pattern = new_new_par(
        1,
        new_receive_par(
            vec![ReceiveBind {
                patterns: vec![prepend_connective(
                    vector_par(Vec::new(), false),
                    Connective {
                        connective_instance: Some(VarRefBody(VarRef { index: 0, depth: 1 })),
                    },
                    0,
                )],
                source: Some(vector_par(Vec::new(), false)),
                remainder: None,
                free_count: 0,
            }],
            vector_par(Vec::new(), false),
            false,
            false,
            0,
            Vec::new(),
            false,
            Vec::new(),
            false,
        ),
        Vec::new(),
        BTreeMap::new(),
        Vec::new(),
        Vec::new(),
        false,
    );

    assert!(assert_spatial_match(target, pattern, Some(BTreeMap::new())).is_ok());
}

#[test]
fn matching_plus_plus_should_work() {
    let target: Par = vector_par(Vec::new(), false).with_exprs(vec![Expr {
        expr_instance: Some(EPlusPlusBody(EPlusPlus {
            p1: Some(new_gstring_par("abc".to_string(), Vec::new(), false)),
            p2: Some(new_gstring_par("def".to_string(), Vec::new(), false)),
        })),
    }]);

    let pattern = vector_par(Vec::new(), true).with_exprs(vec![Expr {
        expr_instance: Some(EPlusPlusBody(EPlusPlus {
            p1: Some(new_freevar_par(0, Vec::new())),
            p2: Some(new_freevar_par(1, Vec::new())),
        })),
    }]);

    let expected_captures = BTreeMap::from([
        (0, new_gstring_par("abc".to_string(), Vec::new(), false)),
        (1, new_gstring_par("def".to_string(), Vec::new(), false)),
    ]);
    assert!(assert_spatial_match(target, pattern, Some(expected_captures)).is_ok());
}

#[test]
fn matching_percent_percent_should_work() {
    let map: Par = vector_par(Vec::new(), false).with_exprs(vec![Expr {
        expr_instance: Some(EMapBody(ParMapTypeMapper::par_map_to_emap(
            ParMap::create_from_vec(vec![(
                new_gstring_par("name".to_string(), Vec::new(), false),
                new_gstring_par("a".to_string(), Vec::new(), false),
            )]),
        ))),
    }]);

    let target = vector_par(Vec::new(), false).with_exprs(vec![Expr {
        expr_instance: Some(EPercentPercentBody(EPercentPercent {
            p1: Some(new_gstring_par("${name}".to_string(), Vec::new(), false)),
            p2: Some(map.clone()),
        })),
    }]);

    let pattern = vector_par(Vec::new(), true).with_exprs(vec![Expr {
        expr_instance: Some(EPercentPercentBody(EPercentPercent {
            p1: Some(new_freevar_par(0, Vec::new())),
            p2: Some(new_freevar_par(1, Vec::new())),
        })),
    }]);

    let expected_captures = BTreeMap::from([
        (0, new_gstring_par("${name}".to_string(), Vec::new(), false)),
        (1, map),
    ]);
    assert!(assert_spatial_match(target, pattern, Some(expected_captures)).is_ok());
}

#[test]
fn matching_minus_minus_should_work() {
    let lhs_set = ESetBody(ParSetTypeMapper::par_set_to_eset(ParSet::new(
        vec![
            new_gint_par(1, Vec::new(), false),
            new_gint_par(2, Vec::new(), false),
            new_gint_par(3, Vec::new(), false),
        ],
        false,
        Vec::new(),
        None,
    )));
    let lhs_set_par: Par = vector_par(Vec::new(), false).with_exprs(vec![Expr {
        expr_instance: Some(ESetBody(ParSetTypeMapper::par_set_to_eset(ParSet::new(
            vec![
                new_gint_par(1, Vec::new(), false),
                new_gint_par(2, Vec::new(), false),
                new_gint_par(3, Vec::new(), false),
            ],
            false,
            Vec::new(),
            None,
        )))),
    }]);

    let rhs_set = ESetBody(ParSetTypeMapper::par_set_to_eset(ParSet::new(
        vec![
            new_gint_par(1, Vec::new(), false),
            new_gint_par(2, Vec::new(), false),
        ],
        false,
        Vec::new(),
        None,
    )));
    let rhs_set_par: Par = vector_par(Vec::new(), false).with_exprs(vec![Expr {
        expr_instance: Some(ESetBody(ParSetTypeMapper::par_set_to_eset(ParSet::new(
            vec![
                new_gint_par(1, Vec::new(), false),
                new_gint_par(2, Vec::new(), false),
            ],
            false,
            Vec::new(),
            None,
        )))),
    }]);

    let target = vector_par(Vec::new(), false).with_exprs(vec![Expr {
        expr_instance: Some(EMinusMinusBody(EMinusMinus {
            p1: Some(vector_par(Vec::new(), false).with_exprs(vec![Expr {
                expr_instance: Some(lhs_set),
            }])),
            p2: Some(vector_par(Vec::new(), false).with_exprs(vec![Expr {
                expr_instance: Some(rhs_set),
            }])),
        })),
    }]);

    let pattern = vector_par(Vec::new(), true).with_exprs(vec![Expr {
        expr_instance: Some(EMinusMinusBody(EMinusMinus {
            p1: Some(new_freevar_par(0, Vec::new())),
            p2: Some(new_freevar_par(1, Vec::new())),
        })),
    }]);

    let expected_captures = BTreeMap::from([(0, lhs_set_par), (1, rhs_set_par)]);
    assert!(assert_spatial_match(target, pattern, Some(expected_captures)).is_ok());
}

#[test]
fn matching_bool_should_work() {
    let success_target = vector_par(Vec::new(), false).with_exprs(vec![Expr {
        expr_instance: Some(GBool(false)),
    }]);

    let fail_target = new_gstring_par("Fail".to_string(), Vec::new(), false);
    let pattern = vector_par(Vec::new(), true).with_connectives(vec![Connective {
        connective_instance: Some(ConnBool(true)),
    }]);

    assert!(assert_spatial_match(success_target, pattern.clone(), Some(BTreeMap::new())).is_ok());
    assert!(assert_spatial_match(fail_target, pattern, None).is_ok());
}

#[test]
fn matching_int_should_work() {
    let success_target = new_gint_par(7, Vec::new(), false);
    let fail_target = new_gstring_par("Fail".to_string(), Vec::new(), false);
    let pattern = vector_par(Vec::new(), true).with_connectives(vec![Connective {
        connective_instance: Some(ConnInt(true)),
    }]);

    assert!(assert_spatial_match(success_target, pattern.clone(), Some(BTreeMap::new())).is_ok());
    assert!(assert_spatial_match(fail_target, pattern, None).is_ok());
}

#[test]
fn matching_string_should_work() {
    let success_target = new_gstring_par("Match me!".to_string(), Vec::new(), false);
    let fail_target = new_gint_par(42, Vec::new(), false);
    let pattern = vector_par(Vec::new(), true).with_connectives(vec![Connective {
        connective_instance: Some(ConnString(true)),
    }]);

    assert!(assert_spatial_match(success_target, pattern.clone(), Some(BTreeMap::new())).is_ok());
    assert!(assert_spatial_match(fail_target, pattern, None).is_ok());
}

#[test]
fn matching_uri_should_work() {
    let success_target = vector_par(Vec::new(), false).with_exprs(vec![Expr {
        expr_instance: Some(GUri("rho:io:stdout".to_string())),
    }]);

    let fail_target = new_gstring_par("Fail".to_string(), Vec::new(), false);
    let pattern = vector_par(Vec::new(), true).with_connectives(vec![Connective {
        connective_instance: Some(ConnUri(true)),
    }]);

    assert!(assert_spatial_match(success_target, pattern.clone(), Some(BTreeMap::new())).is_ok());
    assert!(assert_spatial_match(fail_target, pattern, None).is_ok());
}

#[test]
fn matching_byte_array_should_work() {
    let success_target = vector_par(Vec::new(), false).with_exprs(vec![Expr {
        expr_instance: Some(GByteArray(vec![74, 75])),
    }]);

    let fail_target = new_gstring_par("Fail".to_string(), Vec::new(), false);
    let pattern = vector_par(Vec::new(), true).with_connectives(vec![Connective {
        connective_instance: Some(ConnByteArray(true)),
    }]);

    assert!(assert_spatial_match(success_target, pattern.clone(), Some(BTreeMap::new())).is_ok());
    assert!(assert_spatial_match(fail_target, pattern, None).is_ok());
}

fn par_with_expr(instance: expr::ExprInstance) -> Par {
    vector_par(Vec::new(), false).with_exprs(vec![Expr {
        expr_instance: Some(instance),
    }])
}

fn par_with_connective(instance: connective::ConnectiveInstance) -> Par {
    vector_par(Vec::new(), true).with_connectives(vec![Connective {
        connective_instance: Some(instance),
    }])
}

#[test]
fn matching_int32_should_work() {
    let success_target = par_with_expr(GInt32(-7));
    let fail_target = new_gint_par(7, Vec::new(), false);
    let pattern = par_with_connective(ConnInt32(true));

    assert!(assert_spatial_match(success_target, pattern.clone(), Some(BTreeMap::new())).is_ok());
    assert!(assert_spatial_match(fail_target, pattern, None).is_ok());
}

#[test]
fn matching_uint64_should_work() {
    let success_target = par_with_expr(GUint64(42));
    let fail_target = new_gint_par(42, Vec::new(), false);
    let pattern = par_with_connective(ConnUint64(true));

    assert!(assert_spatial_match(success_target, pattern.clone(), Some(BTreeMap::new())).is_ok());
    assert!(assert_spatial_match(fail_target, pattern, None).is_ok());
}

#[test]
fn matching_uint32_should_work() {
    let success_target = par_with_expr(GUint32(42));
    let fail_target = par_with_expr(GUint64(42));
    let pattern = par_with_connective(ConnUint32(true));

    assert!(assert_spatial_match(success_target, pattern.clone(), Some(BTreeMap::new())).is_ok());
    assert!(assert_spatial_match(fail_target, pattern, None).is_ok());
}

#[test]
fn matching_uint16_should_work() {
    let success_target = par_with_expr(GUint16(42));
    let fail_target = par_with_expr(GUint8(42));
    let pattern = par_with_connective(ConnUint16(true));

    assert!(assert_spatial_match(success_target, pattern.clone(), Some(BTreeMap::new())).is_ok());
    assert!(assert_spatial_match(fail_target, pattern, None).is_ok());
}

#[test]
fn matching_uint8_should_work() {
    let success_target = par_with_expr(GUint8(42));
    let fail_target = par_with_expr(GUint16(42));
    let pattern = par_with_connective(ConnUint8(true));

    assert!(assert_spatial_match(success_target, pattern.clone(), Some(BTreeMap::new())).is_ok());
    assert!(assert_spatial_match(fail_target, pattern, None).is_ok());
}

#[test]
fn matching_float64_should_work() {
    let success_target = par_with_expr(GDouble(1.5f64.to_bits()));
    let fail_target = new_gint_par(1, Vec::new(), false);
    let pattern = par_with_connective(ConnFloat64(true));

    assert!(assert_spatial_match(success_target, pattern.clone(), Some(BTreeMap::new())).is_ok());
    assert!(assert_spatial_match(fail_target, pattern, None).is_ok());
}

#[test]
fn matching_float32_should_work() {
    let success_target = par_with_expr(GFloat32(1.5f32.to_bits()));
    let fail_target = par_with_expr(GDouble(1.5f64.to_bits()));
    let pattern = par_with_connective(ConnFloat32(true));

    assert!(assert_spatial_match(success_target, pattern.clone(), Some(BTreeMap::new())).is_ok());
    assert!(assert_spatial_match(fail_target, pattern, None).is_ok());
}

#[test]
fn matching_big_int_should_work() {
    let success_target = par_with_expr(GBigInt(vec![0x01, 0x00]));
    let fail_target = new_gint_par(256, Vec::new(), false);
    let pattern = par_with_connective(ConnBigInt(true));

    assert!(assert_spatial_match(success_target, pattern.clone(), Some(BTreeMap::new())).is_ok());
    assert!(assert_spatial_match(fail_target, pattern, None).is_ok());
}

#[test]
fn matching_big_rat_should_work() {
    let success_target = par_with_expr(GBigRat(GBigRational {
        numerator: vec![0x01],
        denominator: vec![0x03],
    }));
    let fail_target = par_with_expr(GBigInt(vec![0x01]));
    let pattern = par_with_connective(ConnBigRat(true));

    assert!(assert_spatial_match(success_target, pattern.clone(), Some(BTreeMap::new())).is_ok());
    assert!(assert_spatial_match(fail_target, pattern, None).is_ok());
}

// The i64-only `Int` connective must not match i32, u64, bigint, etc.
#[test]
fn int_does_not_cross_match_other_numeric_types() {
    let int_pattern = par_with_connective(ConnInt(true));

    let candidates = vec![
        par_with_expr(GInt32(1)),
        par_with_expr(GUint64(1)),
        par_with_expr(GUint32(1)),
        par_with_expr(GUint16(1)),
        par_with_expr(GUint8(1)),
        par_with_expr(GDouble(1.0f64.to_bits())),
        par_with_expr(GFloat32(1.0f32.to_bits())),
        par_with_expr(GBigInt(vec![0x01])),
    ];

    for cand in candidates {
        assert!(assert_spatial_match(cand, int_pattern.clone(), None).is_ok());
    }
}

// Helper: build a Par that is a single GFixedPoint with the given unscaled
// bytes and decimal scale.
fn gfp_par(unscaled: Vec<u8>, scale: u32) -> Par {
    par_with_expr(GFixedPoint(models::rhoapi::GFixedPoint { unscaled, scale }))
}

fn fp_pattern(scale: Par) -> Par {
    par_with_connective(ConnFixedPointBody(models::rhoapi::ConnFixedPointBody {
        scale: Some(scale),
    }))
}

#[test]
fn matching_fixed_point_exact_scale() {
    let pattern = fp_pattern(new_gint_par(2, Vec::new(), false));

    let scale2_target = gfp_par(vec![0x96], 2);
    let scale3_target = gfp_par(vec![0x96], 3);
    assert!(assert_spatial_match(scale2_target, pattern.clone(), Some(BTreeMap::new())).is_ok());
    assert!(assert_spatial_match(scale3_target, pattern, None).is_ok());
}

#[test]
fn matching_fixed_point_wildcard_scale() {
    let pattern = fp_pattern(new_wildcard_par(Vec::new(), true));

    let scale0 = gfp_par(vec![0x01], 0);
    let scale18 = gfp_par(vec![0x01], 18);
    assert!(assert_spatial_match(scale0, pattern.clone(), Some(BTreeMap::new())).is_ok());
    assert!(assert_spatial_match(scale18, pattern, Some(BTreeMap::new())).is_ok());
}

#[test]
fn matching_fixed_point_binds_scale() {
    // Pattern: FixedPoint(s) with `s` free at level 0.
    let pattern = fp_pattern(new_freevar_par(0, Vec::new()));

    let target = gfp_par(vec![0x96], 2);
    let expected: BTreeMap<i32, Par> = BTreeMap::from([(0, new_gint_par(2, Vec::new(), false))]);
    assert!(assert_spatial_match(target, pattern, Some(expected)).is_ok());
}

#[test]
fn fixed_point_rejects_non_fixed_point_target() {
    let pattern = fp_pattern(new_wildcard_par(Vec::new(), true));

    let non_fp = new_gint_par(42, Vec::new(), false);
    assert!(assert_spatial_match(non_fp, pattern, None).is_ok());
}

// CoC Phase 4.1b: ConnList pattern matching.
mod coc_list_patterns {
    use models::rhoapi::expr::ExprInstance;

    use super::*;

    fn list_par(elements: Vec<Par>) -> Par {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                ps: elements,
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        }])
    }

    fn conn_list_pattern(element_pattern: Par) -> Par {
        let conn_used = element_pattern.connective_used || true;
        let mut par = Par::default().with_connectives(vec![Connective {
            connective_instance: Some(connective::ConnectiveInstance::ConnList(element_pattern)),
        }]);
        par.connective_used = conn_used;
        par
    }

    fn int_type_pattern() -> Par {
        let mut par = Par::default().with_connectives(vec![Connective {
            connective_instance: Some(connective::ConnectiveInstance::ConnInt(true)),
        }]);
        par.connective_used = true;
        par
    }

    #[test]
    fn homogeneous_int_list_matches_list_of_int_pattern() {
        let target = list_par(vec![
            new_gint_par(1, Vec::new(), false),
            new_gint_par(2, Vec::new(), false),
            new_gint_par(3, Vec::new(), false),
        ]);
        let pattern = conn_list_pattern(int_type_pattern());
        assert!(assert_spatial_match(target, pattern, None).is_ok());
    }

    #[test]
    fn empty_list_matches_list_of_any_pattern() {
        let target = list_par(Vec::new());
        let pattern = conn_list_pattern(int_type_pattern());
        assert!(assert_spatial_match(target, pattern, None).is_ok());
    }

    #[test]
    fn mixed_list_rejects_list_of_int_pattern() {
        let target = list_par(vec![
            new_gint_par(1, Vec::new(), false),
            new_gstring_par("x".to_string(), Vec::new(), false),
        ]);
        let pattern = conn_list_pattern(int_type_pattern());
        assert!(assert_spatial_match(target, pattern, None).is_ok());
    }

    #[test]
    fn non_list_target_rejects_list_pattern() {
        let target = new_gint_par(42, Vec::new(), false);
        let pattern = conn_list_pattern(int_type_pattern());
        assert!(assert_spatial_match(target, pattern, None).is_ok());
    }

    fn set_par(elements: Vec<Par>) -> Par {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::ESetBody(ESet {
                ps: elements,
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        }])
    }

    fn conn_set_pattern(element_pattern: Par) -> Par {
        let mut par = Par::default().with_connectives(vec![Connective {
            connective_instance: Some(connective::ConnectiveInstance::ConnSet(element_pattern)),
        }]);
        par.connective_used = true;
        par
    }

    #[test]
    fn homogeneous_set_matches_set_of_int_pattern() {
        let target = set_par(vec![
            new_gint_par(1, Vec::new(), false),
            new_gint_par(2, Vec::new(), false),
        ]);
        let pattern = conn_set_pattern(int_type_pattern());
        assert!(assert_spatial_match(target, pattern, None).is_ok());
    }

    #[test]
    fn list_target_rejects_set_pattern() {
        let target = list_par(vec![new_gint_par(1, Vec::new(), false)]);
        let pattern = conn_set_pattern(int_type_pattern());
        assert!(assert_spatial_match(target, pattern, None).is_ok());
    }

    fn string_type_pattern() -> Par {
        let mut par = Par::default().with_connectives(vec![Connective {
            connective_instance: Some(connective::ConnectiveInstance::ConnString(true)),
        }]);
        par.connective_used = true;
        par
    }

    fn map_par(kvs: Vec<(Par, Par)>) -> Par {
        let entries: Vec<KeyValuePair> = kvs
            .into_iter()
            .map(|(k, v)| KeyValuePair {
                key: Some(k),
                value: Some(v),
            })
            .collect();
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EMapBody(EMap {
                kvs: entries,
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        }])
    }

    fn conn_map_pattern(key: Par, value: Par) -> Par {
        let mut par = Par::default().with_connectives(vec![Connective {
            connective_instance: Some(connective::ConnectiveInstance::ConnMap(
                models::rhoapi::ConnMapBody {
                    key: Some(key),
                    value: Some(value),
                },
            )),
        }]);
        par.connective_used = true;
        par
    }

    #[test]
    fn map_target_matches_map_string_int_pattern() {
        let target = map_par(vec![
            (
                new_gstring_par("a".to_string(), Vec::new(), false),
                new_gint_par(1, Vec::new(), false),
            ),
            (
                new_gstring_par("b".to_string(), Vec::new(), false),
                new_gint_par(2, Vec::new(), false),
            ),
        ]);
        let pattern = conn_map_pattern(string_type_pattern(), int_type_pattern());
        assert!(assert_spatial_match(target, pattern, None).is_ok());
    }

    #[test]
    fn map_target_rejects_map_pattern_with_wrong_value_type() {
        let target = map_par(vec![(
            new_gstring_par("a".to_string(), Vec::new(), false),
            new_gstring_par("not-an-int".to_string(), Vec::new(), false),
        )]);
        let pattern = conn_map_pattern(string_type_pattern(), int_type_pattern());
        assert!(assert_spatial_match(target, pattern, None).is_ok());
    }

    fn tuple_par(elements: Vec<Par>) -> Par {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::ETupleBody(ETuple {
                ps: elements,
                locally_free: Vec::new(),
                connective_used: false,
            })),
        }])
    }

    fn conn_tuple_pattern(elements: Vec<Par>) -> Par {
        let mut par = Par::default().with_connectives(vec![Connective {
            connective_instance: Some(connective::ConnectiveInstance::ConnTuple(
                models::rhoapi::ConnTupleBody { elements },
            )),
        }]);
        par.connective_used = true;
        par
    }

    #[test]
    fn tuple_target_matches_tuple_pattern_of_matching_shape() {
        let target = tuple_par(vec![
            new_gint_par(1, Vec::new(), false),
            new_gstring_par("x".to_string(), Vec::new(), false),
        ]);
        let pattern = conn_tuple_pattern(vec![int_type_pattern(), string_type_pattern()]);
        assert!(assert_spatial_match(target, pattern, None).is_ok());
    }

    #[test]
    fn tuple_target_rejects_pattern_of_wrong_arity() {
        let target = tuple_par(vec![
            new_gint_par(1, Vec::new(), false),
            new_gstring_par("x".to_string(), Vec::new(), false),
        ]);
        let pattern = conn_tuple_pattern(vec![int_type_pattern()]);
        assert!(assert_spatial_match(target, pattern, None).is_ok());
    }

    #[test]
    fn tuple_target_rejects_pattern_with_wrong_slot_type() {
        let target = tuple_par(vec![
            new_gint_par(1, Vec::new(), false),
            new_gstring_par("x".to_string(), Vec::new(), false),
        ]);
        let pattern = conn_tuple_pattern(vec![string_type_pattern(), int_type_pattern()]);
        assert!(assert_spatial_match(target, pattern, None).is_ok());
    }
}

// CoC Phase 3.1: Pi-shaped patterns typecheck via the typechecker.
mod coc_type_patterns {
    use models::rhoapi::expr::ExprInstance;

    use super::*;

    fn type_par_with(instance: connective::ConnectiveInstance) -> Par {
        let mut par = Par::default().with_connectives(vec![Connective {
            connective_instance: Some(instance),
        }]);
        par.connective_used = true;
        par
    }

    fn pi_pattern(param_ty: Par, codomain: Par) -> Par {
        let conn_used = param_ty.connective_used || codomain.connective_used;
        let mut par = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EPiBody(EPi {
                bind_count: 1,
                param_types: vec![param_ty],
                codomain: Some(codomain),
                locally_free: Vec::new(),
                connective_used: conn_used,
            })),
        }]);
        par.connective_used = conn_used;
        par
    }

    fn bvar_par(k: i32) -> Par {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EVarBody(EVar {
                v: Some(Var {
                    var_instance: Some(var::VarInstance::BoundVar(k)),
                }),
            })),
        }])
    }

    fn lambda_typed_par(bind_count: i32, body: Par, param_types: Vec<Par>) -> Par {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::ELambdaBody(ELambda {
                bind_count,
                body: Some(body),
                locally_free: Vec::new(),
                connective_used: false,
                param_types,
            })),
        }])
    }

    #[test]
    fn annotated_lambda_matches_corresponding_pi_pattern() {
        let int_ty = type_par_with(connective::ConnectiveInstance::ConnInt(true));
        let pattern = pi_pattern(int_ty.clone(), int_ty.clone());
        let lambda = lambda_typed_par(1, bvar_par(0), vec![int_ty]);
        assert!(assert_spatial_match(lambda, pattern, None).is_ok());
    }

    #[test]
    fn annotated_lambda_rejects_mismatched_pi_pattern() {
        let int_ty = type_par_with(connective::ConnectiveInstance::ConnInt(true));
        let bool_ty = type_par_with(connective::ConnectiveInstance::ConnBool(true));
        // Pattern says Int -> Bool but lambda is (Int) -> Int.
        let pattern = pi_pattern(int_ty.clone(), bool_ty);
        let lambda = lambda_typed_par(1, bvar_par(0), vec![int_ty]);
        assert!(assert_spatial_match(lambda, pattern, None).is_ok());
    }

    #[test]
    fn ground_int_rejects_pi_pattern() {
        let int_ty = type_par_with(connective::ConnectiveInstance::ConnInt(true));
        let pattern = pi_pattern(int_ty.clone(), int_ty);
        let not_a_function = new_gint_par(42, Vec::new(), false);
        assert!(assert_spatial_match(not_a_function, pattern, None).is_ok());
    }
}
