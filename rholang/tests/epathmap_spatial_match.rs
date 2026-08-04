//! Consensus-visible spatial-matching laws for homogeneous EPathMaps.
//!
//! These tests exercise the production matcher through `Par`; they do not
//! reach into the private trie matcher. In particular, they pin the neutral
//! empty representation, set/map specialization, exact-key subtraction, and
//! remainder reconstruction without projecting either trie to `Vec<Par>`.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EPathMap, Expr, Par};
use models::rust::epathmap_trie_codec::EPathMapMode;
use models::rust::rholang::implicits::vector_par;
use models::rust::utils::{new_freevar_par, new_freevar_var, new_gint_par, new_wildcard_var};
use rholang::rust::interpreter::matcher::spatial_matcher::SpatialMatcherContext;

fn gint(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

fn carrier(pathmap: EPathMap, connective_used: bool) -> Par {
    vector_par(Vec::new(), connective_used).with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EPathmapBody(pathmap)),
    }])
}

fn bound_pathmap(par: &Par) -> &EPathMap {
    match par.exprs.as_slice() {
        [Expr {
            expr_instance: Some(ExprInstance::EPathmapBody(pathmap)),
        }] => pathmap,
        other => panic!("expected one bound EPathMap expression, got {other:?}"),
    }
}

#[test]
fn set_match_subtracts_exact_keys_and_rebuilds_a_set_remainder() {
    let target_map = EPathMap::new([gint(1), gint(2), gint(3)].to_vec(), vec![], false, None);
    let pattern_map = EPathMap::new(
        [gint(1), new_freevar_par(0, Vec::new())].to_vec(),
        vec![],
        true,
        Some(new_freevar_var(1)),
    );

    let mut matcher = SpatialMatcherContext::new();
    assert!(matcher
        .spatial_match_result(carrier(target_map, false), carrier(pattern_map, true))
        .is_some());

    let explicit = matcher.free_map.get(&0).expect("free set member binds");
    let remainder = bound_pathmap(
        matcher
            .free_map
            .get(&1)
            .expect("free EPathMap remainder binds"),
    );
    assert_eq!(remainder.mode(), EPathMapMode::Set);
    assert_eq!(remainder.len(), 1);

    let explicit_is_two = explicit == &gint(2);
    let explicit_is_three = explicit == &gint(3);
    assert!(explicit_is_two || explicit_is_three);
    assert_eq!(remainder.contains_entry(&gint(2)), explicit_is_three);
    assert_eq!(remainder.contains_entry(&gint(3)), explicit_is_two);
    assert!(!remainder.contains_entry(&gint(1)));
}

#[test]
fn map_match_subtracts_exact_pairs_and_binds_key_and_value() {
    let target_map = EPathMap::new_map(
        [(gint(1), gint(10)), (gint(2), gint(20))],
        vec![],
        false,
        None,
    );
    let pattern_map = EPathMap::new_map(
        [
            (gint(1), gint(10)),
            (
                new_freevar_par(0, Vec::new()),
                new_freevar_par(1, Vec::new()),
            ),
        ],
        vec![],
        true,
        None,
    );

    let mut matcher = SpatialMatcherContext::new();
    assert!(matcher
        .spatial_match_result(carrier(target_map, false), carrier(pattern_map, true))
        .is_some());
    assert_eq!(matcher.free_map.get(&0), Some(&gint(2)));
    assert_eq!(matcher.free_map.get(&1), Some(&gint(20)));
}

#[test]
fn set_and_map_modes_never_cross_match() {
    let target_map = EPathMap::new_map([(gint(1), gint(10))], vec![], false, None);
    let pattern_set = EPathMap::new(vec![new_freevar_par(0, Vec::new())], vec![], true, None);

    let mut matcher = SpatialMatcherContext::new();
    assert!(matcher
        .spatial_match_result(carrier(target_map, false), carrier(pattern_set, true))
        .is_none());
    assert!(matcher.free_map.is_empty());
}

#[test]
fn neutral_empty_is_specialized_only_by_the_value_it_captures() {
    let empty_pattern = EPathMap::new(Vec::<Par>::new(), vec![], true, None);
    let mut matcher = SpatialMatcherContext::new();
    assert!(matcher
        .spatial_match_result(
            carrier(EPathMap::default(), false),
            carrier(empty_pattern, true),
        )
        .is_some());

    let nonempty = EPathMap::new(vec![gint(1)], vec![], false, None);
    let empty_pattern = EPathMap::new(Vec::<Par>::new(), vec![], true, None);
    let mut matcher = SpatialMatcherContext::new();
    assert!(matcher
        .spatial_match_result(carrier(nonempty, false), carrier(empty_pattern, true))
        .is_none());

    let target_map = EPathMap::new_map(
        [(gint(1), gint(10)), (gint(2), gint(20))],
        vec![],
        false,
        None,
    );
    let remainder_pattern =
        EPathMap::new(Vec::<Par>::new(), vec![], true, Some(new_freevar_var(0)));
    let mut matcher = SpatialMatcherContext::new();
    assert!(matcher
        .spatial_match_result(carrier(target_map, false), carrier(remainder_pattern, true))
        .is_some());
    let captured = bound_pathmap(matcher.free_map.get(&0).expect("remainder binds"));
    assert_eq!(captured.mode(), EPathMapMode::Map);
    assert_eq!(captured.len(), 2);
    assert_eq!(captured.get_map_value(&gint(1)).unwrap(), Some(&gint(10)));
    assert_eq!(captured.get_map_value(&gint(2)).unwrap(), Some(&gint(20)));
}

#[test]
fn wildcard_remainder_accepts_leftovers_without_materializing_them() {
    let target = EPathMap::new([gint(1), gint(2)].to_vec(), vec![], false, None);
    let pattern = EPathMap::new(vec![gint(1)], vec![], true, Some(new_wildcard_var()));
    let mut matcher = SpatialMatcherContext::new();
    assert!(matcher
        .spatial_match_result(carrier(target, false), carrier(pattern, true))
        .is_some());
    assert!(matcher.free_map.is_empty());
}
