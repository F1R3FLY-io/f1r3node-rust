//! Byte-exact oracle checks for the final-root-only deep test fixtures.
//!
//! The legacy constructors are deliberately retained here, outside production
//! sources and outside the stack-ceiling driver.  Bounded depths make them a
//! suitable differential oracle without letting their repeated suffix
//! materialization dominate ordinary test discovery or the deep gate itself.

use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{
    EList, EMap, ENot, EPathMap, ESet, Expr, KeyValuePair, New, Par, Receive, ReceiveBind, Send,
};
use models::rust::utils::{new_freevar_par, new_gbool_par, new_gint_par};

#[path = "support/planned_deep_fixture.rs"]
mod planned;

fn legacy_list_pattern(depth: usize) -> Par {
    let mut value = new_freevar_par(0, Vec::new());
    for _ in 0..depth {
        value = models::par_from_default! {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::EListBody(EList {
                    ps: vec![value],
                    locally_free: Vec::new(),
                    connective_used: true,
                    remainder: None,
                })),
            }],
            connective_used: true,
            ..Default::default()
        };
    }
    value
}

fn legacy_sets(depth: usize, leaf: i64) -> Par {
    let mut value = new_gint_par(leaf, Vec::new(), false);
    for _ in 0..depth {
        value = models::par_from_default! {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::ESetBody(ESet {
                    ps: vec![value],
                    locally_free: Vec::new(),
                    connective_used: false,
                    remainder: None,
                })),
            }],
            ..Default::default()
        };
    }
    value
}

fn legacy_maps(depth: usize) -> Par {
    let mut value = new_gint_par(0, Vec::new(), false);
    for _ in 0..depth {
        value = models::par_from_default! {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::EMapBody(EMap {
                    kvs: vec![KeyValuePair {
                        key: Some(value),
                        value: Some(new_gint_par(1, Vec::new(), false)),
                    }],
                    locally_free: Vec::new(),
                    connective_used: false,
                    remainder: None,
                })),
            }],
            ..Default::default()
        };
    }
    value
}

fn legacy_nots(depth: usize) -> Par {
    let mut value = new_gbool_par(true, Vec::new(), false);
    for _ in 0..depth {
        value = models::par_from_default! {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::ENotBody(ENot { p: Some(value) })),
            }],
            ..Default::default()
        };
    }
    value
}

fn legacy_binders(depth: usize) -> Par {
    let mut value = new_gint_par(0, Vec::new(), false);
    for _ in 0..depth {
        let receive = Receive {
            binds: vec![ReceiveBind {
                patterns: vec![Par::default()],
                source: Some(new_gint_par(0, Vec::new(), false)),
                remainder: None,
                free_count: 0,
                cost_region: None,
            }],
            body: Some(value),
            persistent: false,
            peek: false,
            bind_count: 1,
            locally_free: Vec::new(),
            connective_used: false,
            condition: None,
            cost_region: None,
        };
        value = models::par_from_default! {
            news: vec![New {
                bind_count: 1,
                p: Some(models::par_from_default! {
                    receives: vec![receive],
                    ..Default::default()
                }),
                uri: Vec::new(),
                injections: BTreeMap::new(),
                locally_free: Vec::new(),
            }],
            ..Default::default()
        };
    }
    value
}

fn legacy_send_chain(depth: usize) -> Par {
    let mut value = new_gint_par(0, Vec::new(), false);
    for level in 0..depth {
        value = models::par_from_default! {
            sends: vec![Send {
                chan: Some(value),
                data: Vec::new(),
                persistent: false,
                locally_free: vec![(level % 251) as u8],
                connective_used: false,
                cost_region: None,
            }],
            ..Default::default()
        };
    }
    value
}

fn legacy_pathmap_chain(depth: usize, leaf: i64) -> Par {
    let mut value = new_gint_par(leaf, Vec::new(), false);
    for level in 0..depth {
        let key = new_gint_par(level as i64, Vec::new(), false);
        value = models::par_from_default! {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::EPathmapBody(EPathMap::new_map(
                    [(key, value)],
                    vec![(level % 251) as u8],
                    false,
                    None,
                ))),
            }],
            ..Default::default()
        };
    }
    value
}

fn legacy_pathmap_pattern(depth: usize) -> Par {
    let mut value = new_freevar_par(0, Vec::new());
    for level in 0..depth {
        let key = new_gint_par(level as i64, Vec::new(), false);
        value = models::par_from_default! {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::EPathmapBody(EPathMap::new_map(
                    [(key, value)],
                    Vec::new(),
                    true,
                    None,
                ))),
            }],
            connective_used: true,
            ..Default::default()
        };
    }
    value
}

fn digest(value: &Par) -> u64 {
    let mut hash = DefaultHasher::new();
    value.hash(&mut hash);
    hash.finish()
}

fn assert_exact(label: &str, actual: Par, expected: Par) {
    assert_eq!(actual, expected, "{label}: semantic Par differs");
    assert_eq!(
        actual.protobuf_bytes().expect("encode planned fixture"),
        expected.protobuf_bytes().expect("encode oracle fixture"),
        "{label}: canonical protobuf differs"
    );
    assert_eq!(
        actual.compact_snapshot(),
        expected.compact_snapshot(),
        "{label}: compact ParBag snapshot differs"
    );
    assert_eq!(digest(&actual), digest(&expected), "{label}: Hash differs");
    assert_eq!(
        actual.cmp(&expected),
        std::cmp::Ordering::Equal,
        "{label}: Ord differs"
    );
}

#[test]
fn planned_builders_are_byte_exact_with_the_bounded_legacy_oracles() {
    for depth in [0usize, 1, 2, 8, 32] {
        assert_exact(
            "list pattern",
            planned::nested_list_pattern(depth),
            legacy_list_pattern(depth),
        );
        assert_exact(
            "ESet",
            planned::nested_sets_leaf(depth, -7),
            legacy_sets(depth, -7),
        );
        assert_exact("EMap", planned::nested_maps(depth), legacy_maps(depth));
        assert_exact("ENot", planned::nested_nots(depth), legacy_nots(depth));
        assert_exact(
            "binder",
            planned::nested_binders(depth),
            legacy_binders(depth),
        );
        assert_exact(
            "Send",
            planned::nested_send_chain(depth),
            legacy_send_chain(depth),
        );
        assert_exact(
            "EPathMap value chain",
            planned::nested_pathmap_chain_leaf(depth, -7),
            legacy_pathmap_chain(depth, -7),
        );
        assert_exact(
            "EPathMap match pattern",
            planned::nested_pathmap_match_pattern(depth),
            legacy_pathmap_pattern(depth),
        );
    }
}

#[test]
fn final_root_output_value_fixture_matches_frozen_resident_wrapping() {
    for depth in [0usize, 1, 2, 15, 16, 17, 31, 32, 33, 34, 64, 127, 128] {
        let mut expected = new_gint_par(0, Vec::new(), false);
        for _ in 0..depth {
            expected = Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::EListBody(EList {
                    ps: vec![expected],
                    locally_free: Vec::new(),
                    connective_used: false,
                    remainder: None,
                })),
            }]);
        }
        let actual = Par::wrap_unary_elist(depth, new_gint_par(0, Vec::new(), false)).unwrap();
        assert_eq!(actual.canonical_unary_elist_gint_parts(), Some((depth, 0)));
        assert_eq!(actual.locally_free(), expected.locally_free());
        assert_eq!(actual.connective_used(), expected.connective_used());
        assert_eq!(actual.cardinality(), expected.cardinality());
        assert_eq!(
            models::rust::rholang::bincode_encoder::encode(&actual),
            models::rust::rholang::bincode_encoder::encode(&expected)
        );
        assert_exact("output_value unary fixture", actual, expected);
    }
}

#[test]
fn byte_native_witnesses_measure_every_planned_fixture_family() {
    for depth in [0usize, 1, 2, 8, 32, 256] {
        assert_eq!(
            planned::list_pattern_depth(&planned::nested_list_pattern(depth)),
            depth
        );
        assert_eq!(
            planned::eset_depth(&planned::nested_sets_leaf(depth, 0)),
            depth
        );
        assert_eq!(planned::emap_depth(&planned::nested_maps(depth)), depth);
        assert_eq!(planned::enot_depth(&planned::nested_nots(depth)), depth);
        assert_eq!(
            planned::binder_depth(&planned::nested_binders(depth)),
            depth
        );
        assert_eq!(
            planned::send_chain_depth(&planned::nested_send_chain(depth)),
            depth
        );
        assert_eq!(
            planned::pathmap_chain_depth(&planned::nested_pathmap_chain_leaf(depth, 0)),
            depth
        );
        assert_eq!(
            planned::pathmap_chain_depth(&planned::nested_pathmap_match_pattern(depth)),
            depth
        );
    }
}
