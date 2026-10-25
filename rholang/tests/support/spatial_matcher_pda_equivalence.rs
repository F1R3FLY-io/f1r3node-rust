//! Differential between the production PDA and the exact pre-PDA matcher.

#[path = "../../../models/tests/par_corpus/mod.rs"]
mod par_corpus;
#[allow(dead_code)]
#[path = "planned_deep_fixture.rs"]
mod planned_deep_fixture;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use models::rhoapi::connective::ConnectiveInstance::{
    ConnAndBody, ConnBool, ConnByteArray, ConnInt, ConnNotBody, ConnOrBody, ConnString, ConnUri,
    VarRefBody,
};
use models::rhoapi::expr::ExprInstance::EPathmapBody;
use models::rhoapi::{
    Bundle, Connective, ConnectiveBody, CostBoundSignature, CostClauseRegion, CostRegion,
    CostSignature, CostSignatureAtom, CostTokenStack, DeployId, DeployerId, EPathMap, ETuple, Expr,
    GBigRational, GDeployId, GFixedPoint, GUnforgeable, If, KeyValuePair, Match, MatchCase, New,
    PCost, Par, Receive, ReceiveBind, Send, Var, VarRef,
};
use models::rust::par_bag::ParBag;
use models::rust::par_construction::ParConstruction;
use models::rust::rholang::match_path::{encode_match_path, encode_par_match_atom};
use models::rust::rholang::match_schema::{RhoPathCursor, RhoPathEvent};
use models::rust::rholang::match_schema_tables::{ParCursor, RHO_MATCH_REGISTRY};
use models::rust::utils::{
    new_elist_par, new_emap_par, new_eset_par, new_etuple_par, new_freevar_expr, new_freevar_par,
    new_gbool_par, new_gbytearray_par, new_gint_par, new_gstring_par, new_guri_par,
    new_wildcard_par,
};
use pathmap::PathMap;
use proptest::prelude::*;
use rspace_plus_plus::rspace::rho_path::{encode_structural_sequence, RhoPathQuery};

use super::free_map_transaction::{
    assert_unwind_probe, panic_after_writes_for_test, unwind_probe_subject, BindingDelta,
    FreeMapTransaction, UnwindRollbackWitness,
};
use super::match_pars::{match_pars, try_match_pars, MatchParsDomainError};
use super::r#match::{rho_path_query_atom, rho_path_query_cursor_atom};
use super::recursive_oracle::spatial_matcher::{
    SpatialMatcher as RecursiveSpatialMatcher, SpatialMatcherContext as RecursiveContext,
};
use super::spatial_matcher::{SpatialMatcher, SpatialMatcherContext};
use super::spatial_matcher_pda::{
    apply_update_deltas, connective_candidate_positions_with_expansion_mode_stats,
    connective_candidate_positions_with_node_count_order_stats,
    connective_candidate_positions_with_stats, encoded_expr_outer_route, if_target_index_shape,
    match_ac_query_acceptance, match_par_list_with_stats, match_par_with_concrete_region_stats,
    match_par_with_stats, match_par_with_transaction_stats, match_send_list_with_remainder_stats,
    match_send_list_with_stats, new_ac_query_acceptance, receive_bind_ac_query_acceptance,
    send_target_index_shape, CandidateExpansionMode, EncodedExprOuterRoute,
};

#[test]
fn encoded_expr_outer_route_rejects_only_proven_different_tags() {
    use EncodedExprOuterRoute::{Delegate, RejectProvenDifferent};

    for target in [None, Some(10), Some(11)] {
        for pattern in [None, Some(10), Some(11)] {
            let expected = match (target, pattern) {
                (Some(target), Some(pattern)) if target != pattern => RejectProvenDifferent,
                _ => Delegate,
            };
            assert_eq!(encoded_expr_outer_route(target, pattern), expected);
        }
    }
}

#[test]
fn concrete_authoritative_equality_bypasses_recursive_projection_at_depth() {
    let target = planned_deep_fixture::nested_binders(512);
    let pattern = planned_deep_fixture::nested_binders(512);
    let mut context = SpatialMatcherContext::new();
    let (result, stats) = match_par_with_concrete_region_stats(&mut context, target, pattern);
    assert_eq!(result, Some(()));
    assert_eq!(stats.authoritative_equal_fast_positives, 1);
    assert_eq!(stats.byte_region_attempts, 0);
    assert_eq!(stats.legacy_fallbacks, 0);
}

#[test]
fn concrete_binder_byte_regions_scale_linearly_when_ignored_metadata_differs() {
    const DEPTH: usize = 256;
    let target = planned_deep_fixture::nested_binders_with_component_metadata(DEPTH, vec![1]);
    let pattern = planned_deep_fixture::nested_binders_with_component_metadata(DEPTH, vec![2]);
    assert_ne!(
        target, pattern,
        "metadata must defeat authoritative equality"
    );

    let mut context = SpatialMatcherContext::new();
    let (result, stats) = match_par_with_concrete_region_stats(&mut context, target, pattern);
    assert_eq!(result, Some(()));
    assert_eq!(stats.authoritative_equal_fast_positives, 0);
    assert_eq!(stats.byte_region_attempts, 1);
    assert_eq!(stats.legacy_fallbacks, 0);
    assert!(stats.par_region_pairs >= DEPTH * 2);
    assert!(stats.par_region_pairs <= DEPTH * 8 + 8);
    assert!(stats.component_region_pairs >= DEPTH * 2);
    assert!(stats.component_region_pairs <= DEPTH * 8 + 8);
    assert!(
        stats.max_pending_tasks <= 8,
        "the chain must retain bounded pending work, got {stats:?}"
    );
}

#[test]
fn dynamic_singleton_epathmap_values_descend_by_shared_regions_at_depth() {
    const DEPTH: usize = 256;
    let target = planned_deep_fixture::nested_pathmap_chain_leaf(DEPTH, 7);
    let pattern = planned_deep_fixture::nested_pathmap_match_pattern(DEPTH);
    let mut context = SpatialMatcherContext::new();
    let (result, stats) = match_par_with_concrete_region_stats(&mut context, target, pattern);
    assert_eq!(result, Some(()));
    assert_eq!(
        context.free_map.get(&0),
        Some(&new_gint_par(7, Vec::new(), false))
    );
    assert_eq!(stats.epathmap_region_levels, DEPTH);
    assert_eq!(stats.epathmap_endpoint_visits, 2);
    assert_eq!(stats.epathmap_region_backing_copies, 2);
    assert_eq!(stats.epathmap_region_cut_attempts, DEPTH * 2);
    assert_eq!(stats.epathmap_direct_expr_decodes, 0);
    assert_eq!(stats.epathmap_full_decode_fallbacks, 0);
    assert_eq!(stats.epathmap_leaf_decodes, 1);
    assert_eq!(stats.legacy_fallbacks, 0);
}

#[test]
fn singleton_epathmap_region_route_preserves_dynamic_key_then_value_bindings() {
    let target = pathmap(
        EPathMap::new_map(
            [(
                new_gint_par(11, Vec::new(), false),
                new_gint_par(22, Vec::new(), false),
            )],
            Vec::new(),
            false,
            None,
        ),
        false,
    );
    let pattern = pathmap(
        EPathMap::new_map(
            [(
                new_freevar_par(0, Vec::new()),
                new_freevar_par(1, Vec::new()),
            )],
            Vec::new(),
            true,
            None,
        ),
        true,
    );
    let mut context = SpatialMatcherContext::new();
    let (result, stats) = match_par_with_concrete_region_stats(&mut context, target, pattern);
    assert_eq!(result, Some(()));
    assert_eq!(
        context.free_map.get(&0),
        Some(&new_gint_par(11, Vec::new(), false))
    );
    assert_eq!(
        context.free_map.get(&1),
        Some(&new_gint_par(22, Vec::new(), false))
    );
    assert_eq!(stats.epathmap_region_levels, 1);
    assert_eq!(stats.epathmap_endpoint_visits, 2);
    assert_eq!(stats.epathmap_region_backing_copies, 2);
    assert_eq!(stats.epathmap_region_cut_attempts, 2);
    assert_eq!(stats.epathmap_direct_expr_decodes, 0);
    assert_eq!(stats.epathmap_full_decode_fallbacks, 0);
    assert_eq!(stats.epathmap_leaf_decodes, 1);
}

fn assert_wide_epathmap_bypasses_region_cutting(target: Par, pattern: Par) {
    let mut oracle = RecursiveContext::new();
    let expected = oracle
        .spatial_match(target.clone(), pattern.clone())
        .is_some();

    let mut context = SpatialMatcherContext::new();
    let (result, stats) = match_par_with_concrete_region_stats(&mut context, target, pattern);
    assert_eq!(result.is_some(), expected);
    assert_eq!(context.free_map, oracle.free_map);
    assert_eq!(stats.epathmap_region_levels, 0);
    assert_eq!(stats.epathmap_endpoint_visits, 2);
    assert_eq!(stats.epathmap_region_preflight_bypasses, 1);
    assert_eq!(stats.epathmap_region_backing_copies, 2);
    assert_eq!(stats.epathmap_region_cut_attempts, 0);
    assert_eq!(stats.epathmap_direct_expr_decodes, 2);
    assert_eq!(
        stats.epathmap_full_decode_fallbacks, 0,
        "the borrowed preflight should enter the generic PDA directly"
    );
}

#[test]
fn wide_set_epathmap_skips_the_map_value_region_cutter() {
    const WIDTH: usize = 64;
    let mut target = EPathMap::default();
    let mut pattern = EPathMap::default();
    for index in 0..WIDTH {
        let entry = new_gint_par(index as i64, Vec::new(), false);
        target.insert_entry(entry.clone());
        pattern.insert_entry(if index + 1 == WIDTH {
            new_freevar_par(0, Vec::new())
        } else {
            entry
        });
    }
    pattern.connective_used = true;
    assert_wide_epathmap_bypasses_region_cutting(pathmap(target, false), pathmap(pattern, true));
}

#[test]
fn wide_map_epathmap_skips_proportional_par_cut_construction() {
    const WIDTH: usize = 64;
    let mut target = EPathMap::default();
    let mut pattern = EPathMap::default();
    for index in 0..WIDTH {
        let key = new_gint_par(index as i64, Vec::new(), false);
        let value = new_gint_par((10_000 + index) as i64, Vec::new(), false);
        target
            .insert_map_entry(key.clone(), value.clone())
            .expect("fresh target remains map-mode");
        pattern
            .insert_map_entry(
                key,
                if index + 1 == WIDTH {
                    new_freevar_par(0, Vec::new())
                } else {
                    value
                },
            )
            .expect("fresh pattern remains map-mode");
    }
    pattern.connective_used = true;
    assert_wide_epathmap_bypasses_region_cutting(pathmap(target, false), pathmap(pattern, true));
}

#[test]
fn ineligible_scalar_target_rejects_before_copying_a_wide_epathmap_pattern() {
    const WIDTH: usize = 512;
    let mut pattern = EPathMap::default();
    for index in 0..WIDTH {
        pattern.insert_entry(new_gint_par(index as i64, Vec::new(), false));
    }
    pattern.connective_used = true;

    let mut context = SpatialMatcherContext::new();
    let (result, stats) = match_par_with_concrete_region_stats(
        &mut context,
        new_gint_par(7, Vec::new(), false),
        pathmap(pattern, true),
    );
    assert_eq!(result, None);
    assert_eq!(stats.epathmap_endpoint_visits, 0);
    assert_eq!(stats.epathmap_region_backing_copies, 0);
    assert_eq!(stats.epathmap_direct_expr_decodes, 0);
    assert_eq!(stats.epathmap_region_cut_attempts, 0);
}

fn assert_mixed_epathmap_route(target: Par, pattern: Par, expected_match: bool) {
    let mut oracle = RecursiveContext::new();
    let expected = oracle
        .spatial_match(target.clone(), pattern.clone())
        .is_some();
    assert_eq!(expected, expected_match);

    let mut context = SpatialMatcherContext::new();
    let (result, stats) = match_par_with_concrete_region_stats(&mut context, target, pattern);
    assert_eq!(result.is_some(), expected);
    assert_eq!(context.free_map, oracle.free_map);
    assert_eq!(stats.epathmap_endpoint_visits, 2);
    assert_eq!(stats.epathmap_region_preflight_bypasses, 1);
    assert_eq!(stats.epathmap_region_backing_copies, 2);
    assert_eq!(stats.epathmap_region_cut_attempts, 0);
    assert_eq!(stats.epathmap_direct_expr_decodes, 2);
    assert_eq!(stats.epathmap_full_decode_fallbacks, 0);
}

#[test]
fn singleton_target_and_remainder_pattern_use_one_mixed_route_visit_each() {
    let key = new_gint_par(1, Vec::new(), false);
    let value = new_gint_par(11, Vec::new(), false);
    let target = EPathMap::new_map([(key.clone(), value.clone())], Vec::new(), false, None);
    let pattern = EPathMap::new_map(
        [(key, value)],
        Vec::new(),
        true,
        Some(models::rust::utils::new_wildcard_var()),
    );
    assert_mixed_epathmap_route(pathmap(target, false), pathmap(pattern, true), true);
}

#[test]
fn singleton_set_and_singleton_map_mode_mismatch_still_rejects() {
    let key = new_gint_par(1, Vec::new(), false);
    let mut target = EPathMap::default();
    target.insert_entry(key.clone());
    let pattern = EPathMap::new_map(
        [(key, new_gint_par(11, Vec::new(), false))],
        Vec::new(),
        false,
        None,
    );
    assert_mixed_epathmap_route(pathmap(target, false), pathmap(pattern, true), false);
}

#[test]
fn wide_target_and_singleton_pattern_preserve_exact_cardinality() {
    let key = |value: i64| new_gint_par(value, Vec::new(), false);
    let value = |value: i64| new_gint_par(10 + value, Vec::new(), false);
    let target = EPathMap::new_map(
        [(key(1), value(1)), (key(2), value(2))],
        Vec::new(),
        false,
        None,
    );
    let pattern = EPathMap::new_map([(key(1), value(1))], Vec::new(), false, None);
    assert_mixed_epathmap_route(pathmap(target, false), pathmap(pattern, true), false);
}

#[test]
fn wide_epathmap_direct_route_restores_entry_freemap_after_late_failure() {
    const WIDTH: usize = 64;
    let mut target_map = EPathMap::default();
    let mut pattern_map = EPathMap::default();
    for index in 0..WIDTH {
        let key = new_gint_par(index as i64, Vec::new(), false);
        let value = new_gint_par((10_000 + index) as i64, Vec::new(), false);
        target_map
            .insert_map_entry(key.clone(), value.clone())
            .expect("fresh target remains map-mode");
        let pattern_value = if index == 0 {
            new_freevar_par(0, Vec::new())
        } else if index + 1 == WIDTH {
            list(vec![new_freevar_par(1, Vec::new())], true)
        } else {
            value
        };
        pattern_map
            .insert_map_entry(key, pattern_value)
            .expect("fresh pattern remains map-mode");
    }
    pattern_map.connective_used = true;
    let target = pathmap(target_map, false);
    let pattern = pathmap(pattern_map, true);

    let mut baseline = BTreeMap::new();
    baseline.insert(
        99,
        new_gstring_par("entry binding".to_owned(), Vec::new(), false),
    );

    let mut transaction_context = SpatialMatcherContext::new();
    transaction_context.free_map = baseline.clone();
    let (result, transaction_stats) =
        match_par_with_transaction_stats(&mut transaction_context, target.clone(), pattern.clone());
    assert_eq!(result, None);
    assert!(
        transaction_stats.writes >= 1,
        "the first dynamic map value must bind before the later value fails"
    );
    assert!(
        transaction_stats.rollback_entries >= 1,
        "the failed match must roll the earlier binding back"
    );
    assert_eq!(transaction_context.free_map, baseline);

    let mut region_context = SpatialMatcherContext::new();
    region_context.free_map = baseline.clone();
    let (result, stats) =
        match_par_with_concrete_region_stats(&mut region_context, target, pattern);
    assert_eq!(result, None);
    assert_eq!(region_context.free_map, baseline);
    assert_eq!(stats.epathmap_endpoint_visits, 2);
    assert_eq!(stats.epathmap_region_preflight_bypasses, 1);
    assert_eq!(stats.epathmap_region_backing_copies, 2);
    assert_eq!(stats.epathmap_region_cut_attempts, 0);
    assert_eq!(stats.epathmap_direct_expr_decodes, 2);
    assert_eq!(stats.epathmap_region_levels, 0);
    assert_eq!(stats.epathmap_full_decode_fallbacks, 0);
}

fn guarded_concrete_receive(condition: Par, peek: bool, locally_free: Vec<u8>) -> Par {
    models::par_from_default! {
        receives: vec![Receive {
            binds: vec![ReceiveBind {
                patterns: vec![Par::default()],
                source: Some(new_gint_par(1, Vec::new(), false)),
                remainder: None,
                free_count: 0,
                cost_region: None,
            }],
            body: Some(new_gint_par(2, Vec::new(), false)),
            persistent: false,
            peek,
            bind_count: 1,
            locally_free,
            connective_used: false,
            condition: Some(condition),
            cost_region: None,
        }],
        ..Default::default()
    }
}

#[test]
fn concrete_region_route_preserves_receive_condition_and_metadata_equations() {
    let target = guarded_concrete_receive(new_gint_par(7, Vec::new(), false), false, vec![1]);
    let pattern = guarded_concrete_receive(new_gint_par(99, Vec::new(), false), false, vec![2]);
    assert!(
        match_pars(&target, &pattern),
        "the legacy concrete equation ignores condition and locally_free"
    );
    let mut context = SpatialMatcherContext::new();
    let (result, stats) = match_par_with_concrete_region_stats(&mut context, target, pattern);
    assert_eq!(result, Some(()));
    assert_eq!(stats.byte_region_attempts, 1);
    assert_eq!(stats.legacy_fallbacks, 0);

    let target = guarded_concrete_receive(new_gint_par(7, Vec::new(), false), false, vec![1]);
    let pattern = guarded_concrete_receive(new_gint_par(99, Vec::new(), false), true, vec![2]);
    assert!(!match_pars(&target, &pattern));
    let mut context = SpatialMatcherContext::new();
    let (result, stats) = match_par_with_concrete_region_stats(&mut context, target, pattern);
    assert_eq!(result, None);
    assert_eq!(stats.byte_region_attempts, 1);
    assert_eq!(stats.legacy_fallbacks, 0);
}

#[test]
fn concrete_region_route_preserves_nested_par_equality_metadata_equation() {
    let mut target_channel = new_gint_par(7, Vec::new(), false);
    target_channel.locally_free = vec![1];
    let mut pattern_channel = target_channel.clone();
    pattern_channel.locally_free = vec![2];
    assert_eq!(
        target_channel, pattern_channel,
        "Par equality deliberately ignores root locally_free"
    );
    let target = send(target_channel, vec![], false);
    let pattern = send(pattern_channel, vec![], false);
    assert!(match_pars(&target, &pattern));

    let mut context = SpatialMatcherContext::new();
    let (result, stats) = match_par_with_concrete_region_stats(&mut context, target, pattern);
    assert_eq!(result, Some(()));
    assert_eq!(stats.byte_region_attempts, 1);
    assert!(stats.par_region_pairs >= 1);
    assert_eq!(stats.legacy_fallbacks, 0);
}

#[test]
fn concrete_receive_region_route_preserves_legacy_zip_equations() {
    let bind = |patterns: Vec<Par>, source: i64| ReceiveBind {
        patterns,
        source: Some(new_gint_par(source, Vec::new(), false)),
        remainder: None,
        free_count: 0,
        cost_region: None,
    };
    let receive = |binds: Vec<ReceiveBind>| {
        models::par_from_default! {
            receives: vec![Receive {
                binds,
                body: Some(new_gint_par(2, Vec::new(), false)),
                persistent: false,
                peek: false,
                bind_count: 1,
                locally_free: Vec::new(),
                connective_used: false,
                condition: None,
                cost_region: None,
            }],
            ..Default::default()
        }
    };
    let target = receive(vec![
        bind(vec![Par::default(), new_gint_par(3, Vec::new(), false)], 1),
        bind(vec![new_gint_par(4, Vec::new(), false)], 5),
    ]);
    let pattern = receive(vec![bind(vec![Par::default()], 1)]);
    assert!(
        match_pars(&target, &pattern),
        "legacy Receive and ReceiveBind equations zip their vectors"
    );

    let mut context = SpatialMatcherContext::new();
    let (result, stats) = match_par_with_concrete_region_stats(&mut context, target, pattern);
    assert_eq!(result, Some(()));
    assert_eq!(stats.byte_region_attempts, 1);
    assert_eq!(stats.legacy_fallbacks, 0);
}

#[test]
fn concrete_region_route_is_differentially_exact_on_the_canonical_par_corpus() {
    let corpus = par_corpus::par_corpus()
        .into_iter()
        .filter(|(_, par)| !par.connective_used())
        .collect::<Vec<_>>();
    let (mut defined_pairs, mut legacy_undefined_pairs) = (0usize, 0usize);
    let mut domain_errors = BTreeMap::<MatchParsDomainError, usize>::new();
    for (target_name, target) in &corpus {
        for (pattern_name, pattern) in &corpus {
            let expected = match try_match_pars(target, pattern) {
                Ok(expected) => expected,
                Err(error) => {
                    // The corpus intentionally contains absent required
                    // fields. The fallible oracle boundary preserves the
                    // historical short-circuit domain without observing an
                    // unwind or rejecting malformed fields it never reaches.
                    legacy_undefined_pairs += 1;
                    *domain_errors.entry(error).or_default() += 1;
                    let mut context = SpatialMatcherContext::new();
                    let _ = match_par_with_concrete_region_stats(
                        &mut context,
                        target.clone(),
                        pattern.clone(),
                    )
                    .0;
                    assert!(
                        context.free_map.is_empty(),
                        "malformed concrete pair changed FreeMap: {target_name} vs {pattern_name}"
                    );
                    continue;
                }
            };
            defined_pairs += 1;
            let mut context = SpatialMatcherContext::new();
            let actual =
                match_par_with_concrete_region_stats(&mut context, target.clone(), pattern.clone())
                    .0
                    .is_some();
            assert_eq!(
                actual, expected,
                "concrete byte-region differential: {target_name} vs {pattern_name}"
            );
        }
    }
    assert!(defined_pairs > 0);
    assert!(
        legacy_undefined_pairs > 0,
        "the corpus guard must retain its absent-required-field controls"
    );
    assert_eq!(
        defined_pairs + legacy_undefined_pairs,
        corpus.len() * corpus.len(),
        "the typed oracle partition must account for every corpus pair"
    );
    println!("legacy concrete matcher domain errors: {domain_errors:?}");
}

#[test]
fn concrete_match_oracle_domain_tracks_reached_required_fields_only() {
    let missing = Par::default().with_news(vec![New::default()]);
    assert_eq!(
        try_match_pars(&missing, &missing),
        Err(MatchParsDomainError::NewBodyTarget),
        "equal headers reach the first absent required child"
    );

    let mismatching_header = Par::default().with_news(vec![New {
        bind_count: 1,
        ..New::default()
    }]);
    assert_eq!(
        try_match_pars(&missing, &mismatching_header),
        Ok(false),
        "a header mismatch short-circuits before either absent child"
    );
}

fn compare(label: &str, target: Par, pattern: Par) {
    compare_with_entry_map(label, target, pattern, BTreeMap::new());
}

fn compare_with_entry_map(label: &str, target: Par, pattern: Par, entry_map: BTreeMap<i32, Par>) {
    let mut recursive = RecursiveContext::new();
    recursive.free_map = entry_map.clone();
    let recursive_verdict = recursive
        .spatial_match(target.clone(), pattern.clone())
        .is_some();
    let recursive_map = recursive.free_map;

    if recursive_verdict {
        let mut captures = PathMap::new();
        if let Some(atom) = rho_path_query_atom(&pattern, &mut captures) {
            let encoded_target = encode_par_match_atom(&target);
            let target_structure = encode_structural_sequence([encoded_target.as_slice()]);
            let query = RhoPathQuery::structural_sequence([atom], false);
            assert!(
                query.matches_structure(&target_structure),
                "{label}: a compiled RhoPath query rejected a recursive-oracle success"
            );
        }
    }

    let mut pda = SpatialMatcherContext::new();
    pda.free_map = entry_map;
    let pda_verdict = pda.spatial_match(target, pattern).is_some();
    assert_eq!(pda_verdict, recursive_verdict, "{label}: verdict");
    assert_eq!(pda.free_map, recursive_map, "{label}: FreeMap");
    assert_binding_bytes(label, &pda.free_map, &recursive_map);
}

fn assert_binding_bytes(
    label: &str,
    actual_map: &BTreeMap<i32, Par>,
    expected_map: &BTreeMap<i32, Par>,
) {
    assert_eq!(
        actual_map.len(),
        expected_map.len(),
        "{label}: binding count"
    );
    for (level, actual) in actual_map {
        let expected = expected_map
            .get(level)
            .expect("the complete FreeMap comparison proved the binding exists");
        let (protobuf_agrees, bincode_agrees) = binding_byte_agreement(actual, expected);
        assert!(protobuf_agrees, "{label}: binding {level} protobuf bytes");
        assert!(bincode_agrees, "{label}: binding {level} bincode bytes");
    }
}

fn binding_byte_agreement(actual: &Par, expected: &Par) -> (bool, bool) {
    (
        models::rust::rholang::protobuf_encoder::encode_to_vec(actual)
            == models::rust::rholang::protobuf_encoder::encode_to_vec(expected),
        models::rust::rholang::bincode_encoder::encode(actual)
            == models::rust::rholang::bincode_encoder::encode(expected),
    )
}

fn cover_generated_query_root(
    case: &str,
    root: ParCursor<'_>,
    observed: &mut BTreeSet<&'static str>,
    compiled: &mut usize,
    declined: &mut usize,
    pending_bag_components: &mut Vec<OwnedGeneratedQueryRoot>,
) {
    for event in RhoPathCursor::new(root) {
        match event {
            RhoPathEvent::EnterNode { cursor, program } => {
                observed.insert(program.type_name);
                let encoded = encode_match_path(cursor);
                let structure = encode_structural_sequence([encoded.as_slice()]);
                let mut captures = PathMap::new();
                if let Some(atom) = rho_path_query_cursor_atom(cursor, true, &mut captures) {
                    let query = RhoPathQuery::structural_sequence([atom], false);
                    assert!(
                        query.matches_structure(&structure),
                        "{case}/{}: compiled generated cursor excludes its own source",
                        program.type_name
                    );
                    *compiled += 1;
                } else {
                    assert!(
                        captures.is_empty(),
                        "{case}/{}: declined cursor query published speculative captures",
                        program.type_name
                    );
                    *declined += 1;
                }
            }
            RhoPathEvent::ParBag(bag) => {
                enqueue_par_bag_components(bag, pending_bag_components);
            }
            _ => {}
        }
    }
}

/// Owned roots decoded one endpoint at a time from an opaque ParBag event.
/// RMP2 deliberately treats a ParBag as one compact token in production, but
/// the generated-query coverage proof must still exercise every schema program
/// hidden behind those tokens. This explicit heap queue follows that ownership
/// boundary without reintroducing recursive test traversal or a retained
/// component vector in production.
enum OwnedGeneratedQueryRoot {
    Send(Send),
    Receive(Receive),
    New(New),
    Expr(Expr),
    Match(Match),
    Unforgeable(GUnforgeable),
    Connective(Connective),
    Bundle(Bundle),
    If(If),
    CostTokenStack(CostTokenStack),
}

impl OwnedGeneratedQueryRoot {
    fn cursor(&self) -> ParCursor<'_> {
        match self {
            Self::Send(value) => ParCursor::from(value),
            Self::Receive(value) => ParCursor::from(value),
            Self::New(value) => ParCursor::from(value),
            Self::Expr(value) => ParCursor::from(value),
            Self::Match(value) => ParCursor::from(value),
            Self::Unforgeable(value) => ParCursor::from(value),
            Self::Connective(value) => ParCursor::from(value),
            Self::Bundle(value) => ParCursor::from(value),
            Self::If(value) => ParCursor::from(value),
            Self::CostTokenStack(value) => ParCursor::from(value),
        }
    }
}

fn enqueue_par_bag_components(bag: &ParBag, pending: &mut Vec<OwnedGeneratedQueryRoot>) {
    macro_rules! enqueue {
        ($ty:ty, $variant:ident) => {
            bag.try_visit_decoded::<$ty, std::convert::Infallible>(|value, _multiplicity| {
                pending.push(OwnedGeneratedQueryRoot::$variant(value));
                Ok(())
            })
            .expect("an authoritative ParBag contains canonical component keys");
        };
    }

    enqueue!(Send, Send);
    enqueue!(Receive, Receive);
    enqueue!(New, New);
    enqueue!(Expr, Expr);
    enqueue!(Match, Match);
    enqueue!(GUnforgeable, Unforgeable);
    enqueue!(Connective, Connective);
    enqueue!(Bundle, Bundle);
    enqueue!(If, If);
    enqueue!(CostTokenStack, CostTokenStack);
}

#[test]
fn compiled_rho_path_query_is_a_superset_of_every_recursive_oracle_success() {
    let expected = RHO_MATCH_REGISTRY
        .iter()
        .map(|(_, program)| program.type_name)
        .collect::<BTreeSet<_>>();
    let mut observed = BTreeSet::new();
    let mut compiled = 0usize;
    let mut declined = 0usize;
    let mut pending_bag_components = Vec::new();

    for (case, par) in par_corpus::par_corpus() {
        cover_generated_query_root(
            &format!("Par/{case}"),
            ParCursor::from(&par),
            &mut observed,
            &mut compiled,
            &mut declined,
            &mut pending_bag_components,
        );
    }
    for (case, root) in par_corpus::list_par_with_random_corpus() {
        cover_generated_query_root(
            &format!("ListParWithRandom/{case}"),
            ParCursor::from(&root),
            &mut observed,
            &mut compiled,
            &mut declined,
            &mut pending_bag_components,
        );
    }
    for (case, root) in par_corpus::bind_pattern_corpus() {
        cover_generated_query_root(
            &format!("BindPattern/{case}"),
            ParCursor::from(&root),
            &mut observed,
            &mut compiled,
            &mut declined,
            &mut pending_bag_components,
        );
    }
    for (case, root) in par_corpus::tagged_continuation_corpus() {
        cover_generated_query_root(
            &format!("TaggedContinuation/{case}"),
            ParCursor::from(&root),
            &mut observed,
            &mut compiled,
            &mut declined,
            &mut pending_bag_components,
        );
    }
    for (case, root) in par_corpus::par_with_random_corpus() {
        cover_generated_query_root(
            &format!("ParWithRandom/{case}"),
            ParCursor::from(&root),
            &mut observed,
            &mut compiled,
            &mut declined,
            &mut pending_bag_components,
        );
    }
    for (case, root) in par_corpus::list_bind_patterns_corpus() {
        cover_generated_query_root(
            &format!("ListBindPatterns/{case}"),
            ParCursor::from(&root),
            &mut observed,
            &mut compiled,
            &mut declined,
            &mut pending_bag_components,
        );
    }

    let cost = PCost::default();
    cover_generated_query_root(
        "PCost/default",
        ParCursor::from(&cost),
        &mut observed,
        &mut compiled,
        &mut declined,
        &mut pending_bag_components,
    );
    let deploy_id = DeployId::default();
    cover_generated_query_root(
        "DeployId/default",
        ParCursor::from(&deploy_id),
        &mut observed,
        &mut compiled,
        &mut declined,
        &mut pending_bag_components,
    );
    let deployer_id = DeployerId::default();
    cover_generated_query_root(
        "DeployerId/default",
        ParCursor::from(&deployer_id),
        &mut observed,
        &mut compiled,
        &mut declined,
        &mut pending_bag_components,
    );

    // These generated roots are not all reachable from the older Par corpus.
    // Keep the schema census total as the matcher registry grows.
    macro_rules! cover_default_root {
        ($type:ty) => {{
            let value = <$type>::default();
            cover_generated_query_root(
                concat!(stringify!($type), "/default"),
                ParCursor::from(&value),
                &mut observed,
                &mut compiled,
                &mut declined,
                &mut pending_bag_components,
            );
        }};
    }
    cover_default_root!(CostBoundSignature);
    cover_default_root!(CostClauseRegion);
    cover_default_root!(CostRegion);
    cover_default_root!(CostSignature);
    cover_default_root!(CostSignatureAtom);
    cover_default_root!(CostTokenStack);

    while let Some(root) = pending_bag_components.pop() {
        let case = format!("ParBag/{}", root.cursor().program().type_name);
        cover_generated_query_root(
            &case,
            root.cursor(),
            &mut observed,
            &mut compiled,
            &mut declined,
            &mut pending_bag_components,
        );
    }

    assert_eq!(
        observed, expected,
        "the query-compiler corpus must cover every generated schema program"
    );
    assert!(
        compiled > expected.len(),
        "the compiler differential must exercise repeated nested programs"
    );
    assert!(
        declined > 0,
        "the corpus must exercise conservative full-domain query fallback"
    );

    compare(
        "formal bridge: ordered nested capture",
        list(vec![new_gint_par(11, Vec::new(), false)], false),
        list(vec![new_freevar_par(0, Vec::new())], true),
    );
    compare(
        "formal bridge: generated Send capture",
        send(
            new_gint_par(7, Vec::new(), false),
            vec![new_gint_par(11, Vec::new(), false)],
            false,
        ),
        send(
            new_freevar_par(0, Vec::new()),
            vec![new_freevar_par(1, Vec::new())],
            true,
        ),
    );
}

#[test]
fn dynamic_direct_match_value_rows_decline_when_new_ignored_fields_differ() {
    let target = New {
        bind_count: 1,
        p: Some(new_gint_par(7, Vec::new(), false)),
        uri: vec!["rho:id:target".to_owned()],
        injections: BTreeMap::from([(
            "target-injection".to_owned(),
            new_gint_par(11, Vec::new(), false),
        )]),
        locally_free: vec![1],
    };
    let pattern = New {
        bind_count: 1,
        p: Some(new_freevar_par(0, Vec::new())),
        uri: vec!["rho:id:pattern".to_owned()],
        injections: BTreeMap::from([(
            "pattern-injection".to_owned(),
            new_gint_par(22, Vec::new(), false),
        )]),
        locally_free: vec![2],
    };

    let mut oracle = RecursiveContext::new();
    assert!(
        oracle
            .spatial_match(target.clone(), pattern.clone())
            .is_some(),
        "the authoritative New equation ignores URI, injections, and locally_free"
    );
    assert_eq!(
        new_ac_query_acceptance(target, pattern),
        None,
        "a dynamic direct MatchValue::New row must fall back to All, never constrain ignored fields"
    );
}

#[test]
fn dynamic_direct_match_value_rows_decline_when_receive_bind_ignored_fields_differ() {
    let target = ReceiveBind {
        patterns: vec![new_gint_par(3, Vec::new(), false)],
        source: Some(new_gint_par(7, Vec::new(), false)),
        remainder: Some(Var::default()),
        free_count: 41,
        cost_region: None,
    };
    let pattern = ReceiveBind {
        patterns: vec![new_gint_par(3, Vec::new(), false)],
        source: Some(new_freevar_par(0, Vec::new())),
        remainder: None,
        free_count: 0,
        cost_region: None,
    };

    let mut oracle = RecursiveContext::new();
    assert!(
        oracle
            .spatial_match(target.clone(), pattern.clone())
            .is_some(),
        "the authoritative ReceiveBind equation ignores remainder and free_count"
    );
    assert_eq!(
        receive_bind_ac_query_acceptance(target, pattern),
        None,
        "a dynamic direct MatchValue::ReceiveBind row must fall back to All, never constrain ignored fields"
    );
}

#[test]
fn dynamic_direct_match_value_rows_decline_when_match_case_ignored_fields_differ() {
    let target = Match {
        target: Some(new_gint_par(1, Vec::new(), false)),
        cases: vec![MatchCase {
            pattern: Some(new_gint_par(2, Vec::new(), false)),
            source: Some(new_gint_par(7, Vec::new(), false)),
            free_count: 41,
            guard: Some(new_gbool_par(true, Vec::new(), false)),
        }],
        locally_free: vec![1],
        connective_used: false,
    };
    let pattern = Match {
        target: Some(new_gint_par(1, Vec::new(), false)),
        cases: vec![MatchCase {
            pattern: Some(new_gint_par(2, Vec::new(), false)),
            source: Some(new_freevar_par(0, Vec::new())),
            free_count: 0,
            guard: None,
        }],
        locally_free: vec![2],
        connective_used: true,
    };

    let mut oracle = RecursiveContext::new();
    assert!(
        oracle
            .spatial_match(target.clone(), pattern.clone())
            .is_some(),
        "the authoritative Match/MatchCase equations ignore case free_count, guard, and root metadata"
    );
    assert_eq!(
        match_ac_query_acceptance(target, pattern),
        None,
        "a dynamic direct MatchValue::Match row must fall back to All, never constrain ignored fields"
    );
}

#[test]
fn ground_direct_match_value_rows_decline_unproved_schema_equations() {
    let new = New {
        bind_count: 1,
        p: Some(new_gint_par(7, Vec::new(), false)),
        uri: vec!["rho:id:ground".to_owned()],
        injections: BTreeMap::new(),
        locally_free: Vec::new(),
    };
    assert_eq!(new_ac_query_acceptance(new.clone(), new), None);

    let bind = ReceiveBind {
        patterns: vec![new_gint_par(3, Vec::new(), false)],
        source: Some(new_gint_par(7, Vec::new(), false)),
        remainder: None,
        free_count: 0,
        cost_region: None,
    };
    assert_eq!(receive_bind_ac_query_acceptance(bind.clone(), bind), None);

    let case = MatchCase {
        pattern: Some(new_gint_par(2, Vec::new(), false)),
        source: Some(new_gint_par(7, Vec::new(), false)),
        free_count: 0,
        guard: None,
    };
    let matched = Match {
        target: Some(new_gint_par(1, Vec::new(), false)),
        cases: vec![case],
        locally_free: Vec::new(),
        connective_used: false,
    };
    assert_eq!(match_ac_query_acceptance(matched.clone(), matched), None);
}

#[test]
fn declined_nonlinear_query_does_not_publish_speculative_captures() {
    let repeated = list(
        vec![
            new_freevar_par(0, Vec::new()),
            new_freevar_par(0, Vec::new()),
        ],
        true,
    );
    let mut captures = PathMap::new();
    assert!(rho_path_query_atom(&repeated, &mut captures).is_none());
    assert!(
        captures.is_empty(),
        "a declined query must roll back its speculative capture branch"
    );

    assert!(rho_path_query_atom(&new_freevar_par(0, Vec::new()), &mut captures).is_some());
    assert_eq!(captures.val_count(), 1);
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

fn inactive_connective(instance: models::rhoapi::connective::ConnectiveInstance) -> Par {
    Par::default().with_connectives(vec![Connective {
        connective_instance: Some(instance),
    }])
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

fn send_value(chan: Par, data: Vec<Par>, connective_used: bool) -> Send {
    Send {
        chan: Some(chan),
        data,
        persistent: false,
        locally_free: Vec::new(),
        connective_used,
        cost_region: None,
    }
}

fn send(chan: Par, data: Vec<Par>, connective_used: bool) -> Par {
    models::par_from_default! {
        sends: vec![send_value(chan, data, connective_used)],
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
                cost_region: None,
            }],
            body: Some(body),
            persistent: false,
            peek: false,
            bind_count: 1,
            locally_free: Vec::new(),
            connective_used,
            condition: None,
            cost_region: None,
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

fn conditional(condition: Par, if_true: Par, if_false: Par, connective_used: bool) -> Par {
    models::par_from_default! {
        conditionals: vec![If {
            condition: Some(condition),
            if_true: Some(if_true),
            if_false: Some(if_false),
            locally_free: Vec::new(),
            connective_used,
        }],
        connective_used,
        ..Default::default()
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn generated_nested_if_equation_order_and_rollback_match_recursive_oracle(
        depth in 1usize..17,
        failure_level in 0usize..16,
        seed in any::<u64>(),
        leaf_value in -1000i64..1000,
    ) {
        let leaf = new_gint_par(leaf_value, Vec::new(), false);
        let mut target = leaf.clone();
        let mut success_pattern = new_freevar_par(0, Vec::new());
        let mut failure_pattern = success_pattern.clone();
        for level in 0..depth {
            let condition_value = 1000 + i64::from(seed.rotate_left(level as u32) as u8);
            let false_value = 2000 + i64::from(seed.rotate_right(level as u32) as u8);
            let condition = new_gint_par(condition_value, Vec::new(), false);
            let false_branch = new_gint_par(false_value, Vec::new(), false);
            let pattern_false = if level == failure_level % depth {
                new_gint_par(false_value + 10_000, Vec::new(), false)
            } else {
                false_branch.clone()
            };
            target = conditional(condition.clone(), target, false_branch, false);
            success_pattern = conditional(
                condition.clone(), success_pattern, new_gint_par(false_value, Vec::new(), false), true,
            );
            failure_pattern = conditional(condition, failure_pattern, pattern_false, true);
        }

        let entry_map = BTreeMap::from([(41, new_gint_par(42, Vec::new(), false))]);
        let mut success_oracle = RecursiveContext::new();
        success_oracle.free_map = entry_map.clone();
        prop_assert!(success_oracle
            .spatial_match(target.clone(), success_pattern.clone())
            .is_some());
        prop_assert_eq!(success_oracle.free_map.get(&0), Some(&leaf));
        prop_assert_eq!(success_oracle.free_map.get(&41), entry_map.get(&41));
        compare_with_entry_map(
            "generated nested If equation order: success",
            target.clone(),
            success_pattern,
            entry_map.clone(),
        );
        let mut failure_oracle = RecursiveContext::new();
        failure_oracle.free_map = entry_map.clone();
        prop_assert!(failure_oracle
            .spatial_match(target.clone(), failure_pattern.clone())
            .is_none());
        prop_assert_eq!(failure_oracle.free_map, entry_map.clone());
        compare_with_entry_map(
            "generated nested If equation order: rollback",
            target,
            failure_pattern,
            entry_map,
        );
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
        "nested tuple fields",
        tuple(vec![seven(), tuple(vec![one(), two()], false)], false),
        tuple(vec![free(0), tuple(vec![free(1), free(2)], true)], true),
    );
    compare(
        "send channel and data",
        send(one(), vec![seven()], false),
        send(free(0), vec![free(1)], true),
    );
    let duplicate = send(one(), vec![seven()], false)
        .first::<Send>()
        .unwrap()
        .unwrap()
        .0;
    let duplicate_target = models::par_from_default! {
        sends: vec![duplicate.clone(), duplicate.clone(), duplicate],
        ..Default::default()
    };
    let mut duplicate_pattern = send(one(), vec![free(0)], true);
    let mut expressions = duplicate_pattern.take_occurrences::<Expr>();
    expressions.push(new_freevar_expr(1));
    duplicate_pattern.replace_exprs(expressions);
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
        "conditional children",
        conditional(
            new_gint_par(11, Vec::new(), false),
            new_gint_par(12, Vec::new(), false),
            new_gint_par(13, Vec::new(), false),
            false,
        ),
        conditional(free(0), free(1), free(2), true),
    );
    compare(
        "conditional child refusal",
        conditional(
            new_gint_par(11, Vec::new(), false),
            new_gint_par(12, Vec::new(), false),
            new_gint_par(13, Vec::new(), false),
            false,
        ),
        conditional(free(0), new_gint_par(99, Vec::new(), false), free(1), true),
    );

    let mut nested_target = seven();
    let mut nested_pattern = free(0);
    for depth in 1..=8 {
        nested_target = conditional(one(), nested_target, two(), false);
        nested_pattern = conditional(one(), nested_pattern, two(), true);
        let mut success_oracle = RecursiveContext::new();
        assert!(
            success_oracle
                .spatial_match(nested_target.clone(), nested_pattern.clone())
                .is_some(),
            "nested conditional depth {depth} must bind in the recursive oracle"
        );
        assert_eq!(success_oracle.free_map.get(&0), Some(&seven()));
        compare(
            &format!("nested conditional depth {depth}"),
            nested_target.clone(),
            nested_pattern.clone(),
        );

        let entry_map = BTreeMap::from([(41, new_gint_par(42, Vec::new(), false))]);
        let refusal_target = conditional(one(), nested_target.clone(), two(), false);
        let refusal_pattern = conditional(
            one(),
            nested_pattern.clone(),
            new_gint_par(99, Vec::new(), false),
            true,
        );
        let mut refusal_oracle = RecursiveContext::new();
        refusal_oracle.free_map = entry_map.clone();
        assert!(
            refusal_oracle
                .spatial_match(refusal_target.clone(), refusal_pattern.clone())
                .is_none(),
            "nested conditional depth {depth} must refuse after the earlier binding"
        );
        assert_eq!(refusal_oracle.free_map, entry_map);
        compare_with_entry_map(
            &format!("nested conditional rollback depth {depth}"),
            refusal_target,
            refusal_pattern,
            entry_map,
        );
    }

    compare(
        "conjunction",
        seven(),
        connective(ConnAndBody(ConnectiveBody {
            ps: vec![free(0), seven()],
        })),
    );
    compare(
        "failed conjunction rolls back every earlier conjunct binding",
        seven(),
        connective(ConnAndBody(ConnectiveBody {
            ps: vec![free(0), two()],
        })),
    );
    compare(
        "disjunction",
        seven(),
        connective(ConnOrBody(ConnectiveBody {
            ps: vec![two(), free(0)],
        })),
    );
    compare(
        "disjunction isolates a failed binding branch before success",
        seven(),
        connective(ConnOrBody(ConnectiveBody {
            ps: vec![
                connective(ConnAndBody(ConnectiveBody {
                    ps: vec![free(0), two()],
                })),
                seven(),
            ],
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
        "legacy set Bag residual",
        new_eset_par(
            vec![one(), two(), seven()],
            Vec::new(),
            false,
            None,
            Vec::new(),
            false,
        ),
        new_eset_par(
            vec![two()],
            Vec::new(),
            true,
            Some(models::rust::utils::new_freevar_var(4)),
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
fn apply_update_deltas_refuses_before_any_write() {
    let baseline_value = new_gstring_par("baseline".into(), Vec::new(), false);
    let mut context = SpatialMatcherContext::new();
    context.free_map.insert(41, baseline_value);
    let baseline = context.free_map.clone();

    let first = BTreeMap::from([(0, new_gint_par(11, Vec::new(), false))]);
    let conflicting = BTreeMap::from([(0, new_gint_par(29, Vec::new(), false))]);
    let second = BTreeMap::from([(1, new_gint_par(29, Vec::new(), false))]);
    let expected =
        super::list_match::aggregate_updates(baseline.clone(), vec![first.clone(), second.clone()])
            .expect("disjoint deltas aggregate");

    let mut transaction = FreeMapTransaction::new(&mut context);
    let before = transaction.journal_len();
    assert!(!apply_update_deltas(&mut transaction, vec![
        BindingDelta::from_map(first.clone()),
        BindingDelta::from_map(conflicting),
    ],));
    assert_eq!(transaction.journal_len(), before);
    assert_eq!(transaction.free_map(), &baseline);

    assert!(apply_update_deltas(&mut transaction, vec![
        BindingDelta::from_map(first),
        BindingDelta::from_map(second),
    ],));
    assert_eq!(transaction.free_map(), &expected);
    assert!(transaction.finish(true).is_some());
    assert_eq!(context.free_map, expected);
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
    assert_eq!(diagonal_stats.target_index_builds, 1);
    assert_eq!(diagonal_stats.edge_evaluations, WIDTH);
    assert!(diagonal_stats.edge_evaluations < diagonal_control);
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
    // Pattern zero has the singleton domain `{1}`. Each interior disjunction
    // has two exact candidates and the last has one in-range candidate, so
    // PathMap union reduces the complete semantic relation to exactly
    // `1 + 2*(WIDTH-2) + 1` evaluated edges. The nominal Cartesian oracle is
    // retained above as the before-work witness.
    let connective_domain_evaluations = 2 * (WIDTH - 1);
    for _ in 0..SAMPLES {
        let mut context = SpatialMatcherContext::new();
        let (result, stats) = match_par_list_with_stats(&mut context, integers(), chain_patterns());
        assert!(result.is_none());
        assert_eq!(stats.edge_evaluations, connective_domain_evaluations);
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
fn production_list_pda_connective_candidate_algebra_is_conservative_and_exactly_ordered() {
    let integer = |value| new_gint_par(value, Vec::new(), false);
    let disjunction = |ps| connective(ConnOrBody(ConnectiveBody { ps }));
    let conjunction = |ps| connective(ConnAndBody(ConnectiveBody { ps }));

    let targets = vec![integer(0), integer(1), integer(4)];
    let patterns = vec![
        disjunction(vec![integer(1), integer(3)]),
        conjunction(vec![
            disjunction(vec![integer(2), integer(4)]),
            disjunction(vec![integer(4), integer(5)]),
        ]),
        integer(0),
    ];
    let mut context = SpatialMatcherContext::new();
    let (result, stats) =
        match_par_list_with_stats(&mut context, targets.clone(), patterns.clone());
    assert!(result.is_some());
    assert_eq!(
        stats.edge_evaluations, 3,
        "union produces [1], nested intersection produces [4], and exact zero produces [0]"
    );

    compare(
        "PathMap connective-domain algebra preserves the recursive oracle",
        new_eset_par(targets, Vec::new(), false, None, Vec::new(), false),
        new_eset_par(patterns, Vec::new(), true, None, Vec::new(), true),
    );

    // Empty OR is authoritative refusal, so its empty derived domain may fail
    // before any broader row. Empty AND is true and therefore deliberately
    // compiles to All rather than to an empty set.
    let mut context = SpatialMatcherContext::new();
    let (result, stats) =
        match_par_list_with_stats(&mut context, vec![integer(0), integer(1)], vec![
            integer(0),
            disjunction(Vec::new()),
        ]);
    assert!(result.is_none());
    assert_eq!(stats.edge_evaluations, 0);

    let mut context = SpatialMatcherContext::new();
    let (result, stats) =
        match_par_list_with_stats(&mut context, vec![integer(0), integer(1)], vec![
            integer(0),
            conjunction(Vec::new()),
        ]);
    assert!(result.is_some());
    assert!(stats.edge_evaluations >= 2);

    // NOT cannot safely complement a conservative query. In OR it annihilates
    // pruning (union with All); in AND the exact positive arm remains a sound
    // necessary condition. Both paths still obtain their verdict from the
    // spatial matcher.
    let negated_two = connective(ConnNotBody(integer(2)));
    let mut context = SpatialMatcherContext::new();
    let (result, or_stats) =
        match_par_list_with_stats(&mut context, vec![integer(0), integer(1)], vec![
            disjunction(vec![integer(9), negated_two.clone()]),
            integer(1),
        ]);
    assert!(result.is_some());
    assert!(or_stats.edge_evaluations >= 2);

    let mut context = SpatialMatcherContext::new();
    let (result, and_stats) =
        match_par_list_with_stats(&mut context, vec![integer(0), integer(1)], vec![
            conjunction(vec![integer(0), negated_two]),
            integer(1),
        ]);
    assert!(result.is_some());
    assert_eq!(and_stats.edge_evaluations, 2);

    // A pure connective carrier is evaluated directly against the whole Par.
    // Proper subsets must not be accepted merely because one connective arm
    // matches them; these compound cases prove the fast transition against
    // the pre-PDA oracle in both directions.
    let mut compound = integer(0);
    let second = integer(1);
    compound = compound.append(second);
    compare(
        "pure OR carrier accepts the whole compound target",
        compound.clone(),
        disjunction(vec![integer(9), compound.clone()]),
    );
    compare(
        "pure OR carrier rejects a matching proper subset",
        compound.clone(),
        disjunction(vec![integer(0)]),
    );
    compare(
        "pure AND carrier constrains the whole compound target",
        compound.clone(),
        conjunction(vec![compound, connective(ConnNotBody(integer(9)))]),
    );
}

#[test]
fn byte_native_connective_authority_covers_every_terminal_shape() {
    for payload in [false, true] {
        compare(
            "encoded Bool predicate",
            new_gbool_par(false, Vec::new(), false),
            connective(ConnBool(payload)),
        );
        compare(
            "encoded Int predicate",
            new_gint_par(-7, Vec::new(), false),
            connective(ConnInt(payload)),
        );
        compare(
            "encoded String predicate",
            new_gstring_par("value".to_owned(), Vec::new(), false),
            connective(ConnString(payload)),
        );
        compare(
            "encoded Uri predicate",
            new_guri_par("rho:id:test".to_owned(), Vec::new(), false),
            connective(ConnUri(payload)),
        );
        compare(
            "encoded ByteArray predicate",
            new_gbytearray_par(vec![0, 1, 0xff], Vec::new(), false),
            connective(ConnByteArray(payload)),
        );
    }

    compare(
        "encoded predicate rejects a different scalar family",
        new_gint_par(1, Vec::new(), false),
        connective(ConnBool(true)),
    );
    compare(
        "empty encoded AND is true",
        new_gint_par(1, Vec::new(), false),
        connective(ConnAndBody(ConnectiveBody { ps: Vec::new() })),
    );
    compare(
        "empty encoded OR is false",
        new_gint_par(1, Vec::new(), false),
        connective(ConnOrBody(ConnectiveBody { ps: Vec::new() })),
    );
    compare(
        "encoded VarRef remains unconditional refusal",
        new_gint_par(1, Vec::new(), false),
        connective(VarRefBody(VarRef { index: 0, depth: 0 })),
    );
    compare(
        "absent encoded Connective remains unconditional refusal",
        new_gint_par(1, Vec::new(), false),
        Par::default().with_connectives(vec![Connective::default()]),
    );
}

#[test]
fn byte_native_connective_respects_child_and_root_dispatch_metadata() {
    let inactive_predicate = inactive_connective(ConnInt(true));
    let integer = new_gint_par(7, Vec::new(), false);

    compare(
        "inactive root Connective remains concrete data",
        inactive_predicate.clone(),
        inactive_predicate.clone(),
    );
    compare(
        "inactive root Connective is not an integer predicate",
        integer.clone(),
        inactive_predicate.clone(),
    );

    let unary = connective(ConnAndBody(ConnectiveBody {
        ps: vec![inactive_predicate.clone()],
    }));
    compare(
        "unary spine stops before an inactive Connective child",
        integer.clone(),
        unary.clone(),
    );
    let mut context = SpatialMatcherContext::new();
    let (result, unary_stats) = match_par_with_stats(&mut context, integer.clone(), unary);
    assert!(result.is_none());
    assert_eq!(
        unary_stats.authoritative_connective_evaluations, 0,
        "the unary normalizer must stop before the inactive child"
    );

    let branching = connective(ConnAndBody(ConnectiveBody {
        ps: vec![inactive_predicate.clone(), inactive_predicate.clone()],
    }));
    compare(
        "branching encoded descent delegates inactive Connective children",
        integer.clone(),
        branching.clone(),
    );
    let mut context = SpatialMatcherContext::new();
    let (result, branching_stats) =
        match_par_with_stats(&mut context, integer.clone(), branching.clone());
    assert!(result.is_none());
    assert_eq!(
        branching_stats.authoritative_connective_evaluations, 1,
        "only the active outer AND may be evaluated as logical syntax"
    );
    compare(
        "direct route preserves legacy exclusion of inactive Connective target",
        inactive_predicate.clone(),
        branching.clone(),
    );

    let metadata_sensitive = connective(ConnAndBody(ConnectiveBody {
        ps: vec![new_freevar_par(0, Vec::new()), connective(ConnInt(true))],
    }));
    compare(
        "direct route preserves locally-free projection before capture",
        integer.clone().with_locally_free(vec![0x01]),
        metadata_sensitive.clone(),
    );
    compare(
        "direct route preserves connective-used projection before capture",
        integer.clone().with_connective_used(true),
        metadata_sensitive,
    );

    for encoded_compiler in [false, true] {
        let (positions, stats) = connective_candidate_positions_with_stats(
            vec![inactive_predicate.clone(), integer.clone()],
            branching.clone(),
            encoded_compiler,
        );
        assert!(
            positions.contains(&0),
            "candidate compilation must retain the concrete inactive-Connective target"
        );
        if encoded_compiler {
            assert!(
                stats.candidate_terminal_par_decodes >= 2,
                "both inactive children must delegate to concrete Par compilation: {stats:?}"
            );
        } else {
            assert_eq!(
                stats.candidate_resident_connective_decodes, 1,
                "only the active outer AND is a resident logical connective: {stats:?}"
            );
        }
    }
}

#[test]
fn exact_free_var_capture_is_byte_native_in_every_and_child_position() {
    let target = new_gint_par(7, Vec::new(), false);
    let predicate = connective(ConnInt(true));
    for (position, children) in [
        ("first", vec![
            new_freevar_par(0, Vec::new()),
            predicate.clone(),
        ]),
        ("middle", vec![
            predicate.clone(),
            new_freevar_par(0, Vec::new()),
            predicate.clone(),
        ]),
        ("final", vec![
            predicate.clone(),
            new_freevar_par(0, Vec::new()),
        ]),
    ] {
        let pattern = connective(ConnAndBody(ConnectiveBody { ps: children }));
        compare(
            &format!("exact FreeVar capture in {position} AND position"),
            target.clone(),
            pattern.clone(),
        );
        let mut context = SpatialMatcherContext::new();
        let (result, stats) = match_par_with_stats(&mut context, target.clone(), pattern);
        assert!(result.is_some(), "{position} capture must match");
        assert_eq!(context.free_map.get(&0), Some(&target));
        assert_eq!(stats.authoritative_exact_free_var_captures, 1);
        assert_eq!(stats.authoritative_terminal_par_decodes, 0);
    }
}

#[test]
fn byte_native_connective_exact_scalar_route_is_complete_and_non_vacuous() {
    use models::rhoapi::expr::ExprInstance;

    fn scalar(instance: ExprInstance) -> Par {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(instance),
        }])
    }
    fn binary_and(pattern: Par) -> Par {
        connective(ConnAndBody(ConnectiveBody {
            ps: vec![pattern.clone(), pattern],
        }))
    }

    let cases = [
        (
            scalar(ExprInstance::GBool(false)),
            scalar(ExprInstance::GBool(true)),
        ),
        (
            scalar(ExprInstance::GInt(i64::MIN)),
            scalar(ExprInstance::GInt(i64::MAX)),
        ),
        (
            scalar(ExprInstance::GString("λ\0".to_owned())),
            scalar(ExprInstance::GString(String::new())),
        ),
        (
            scalar(ExprInstance::GUri("rho:id:λ".to_owned())),
            scalar(ExprInstance::GUri(String::new())),
        ),
        (
            scalar(ExprInstance::GByteArray(vec![0, 1, 0xff])),
            scalar(ExprInstance::GByteArray(Vec::new())),
        ),
        (
            scalar(ExprInstance::GDouble(0x7ff8_0000_0000_0042)),
            scalar(ExprInstance::GDouble(0x7ff8_0000_0000_0043)),
        ),
        (
            scalar(ExprInstance::GBigInt(vec![0x80, 0])),
            scalar(ExprInstance::GBigInt(Vec::new())),
        ),
        (
            scalar(ExprInstance::GBigRat(GBigRational {
                numerator: vec![1],
                denominator: vec![2],
            })),
            scalar(ExprInstance::GBigRat(GBigRational::default())),
        ),
        (
            scalar(ExprInstance::GFixedPoint(GFixedPoint {
                unscaled: vec![1],
                scale: u32::MAX,
            })),
            scalar(ExprInstance::GFixedPoint(GFixedPoint::default())),
        ),
    ];

    for (equal, unequal) in cases {
        compare(
            "exact scalar locally-free metadata equality",
            equal.clone().with_locally_free(vec![0x01]),
            binary_and(equal.clone().with_locally_free(vec![0x80, 0x01])),
        );
        let mut context = SpatialMatcherContext::new();
        let (result, stats) = match_par_with_stats(
            &mut context,
            equal.clone(),
            binary_and(equal.clone().with_locally_free(vec![0x80, 0x01])),
        );
        assert!(
            result.is_some(),
            "child locally-free summaries do not affect equality"
        );
        assert_eq!(
            stats.authoritative_exact_scalar_attempts, 2,
            "equal scalar did not take the proven route: {equal:?}; {stats:?}"
        );
        assert_eq!(stats.authoritative_exact_scalar_equal, 2);
        assert_eq!(stats.authoritative_exact_scalar_mismatch, 0);
        assert_eq!(stats.authoritative_terminal_par_decodes, 0);

        let mut context = SpatialMatcherContext::new();
        let (result, stats) =
            match_par_with_stats(&mut context, unequal, binary_and(equal.clone()));
        assert!(result.is_none());
        assert_eq!(
            stats.authoritative_exact_scalar_attempts, 1,
            "unequal scalar did not take the proven route: {equal:?}; {stats:?}"
        );
        assert_eq!(stats.authoritative_exact_scalar_equal, 0);
        assert_eq!(stats.authoritative_exact_scalar_mismatch, 1);
        assert_eq!(stats.authoritative_terminal_par_decodes, 0);

        compare(
            "exact scalar accepted domain",
            equal.clone(),
            binary_and(equal),
        );
    }

    let mut context = SpatialMatcherContext::new();
    let (result, stats) = match_par_with_stats(
        &mut context,
        new_gint_par(7, Vec::new(), false),
        binary_and(new_gbool_par(true, Vec::new(), false)),
    );
    assert!(result.is_none(), "different scalar tags are unequal");
    assert_eq!(stats.authoritative_exact_scalar_attempts, 1);
    assert_eq!(stats.authoritative_exact_scalar_mismatch, 1);

    let delegated = new_gint_par(7, Vec::new(), false).with_connective_used(true);
    let mut context = SpatialMatcherContext::new();
    let (_, stats) = match_par_with_stats(
        &mut context,
        new_gint_par(7, Vec::new(), false),
        binary_and(delegated),
    );
    assert_eq!(stats.authoritative_exact_scalar_attempts, 0);
    assert_eq!(stats.authoritative_exact_scalar_unproved, 2);
    assert_eq!(stats.authoritative_terminal_par_decodes, 2);
}

fn planned_skewed_connective(depth: usize, conjunction: bool) -> Par {
    let mut construction = ParConstruction::new();
    let terminal = new_gint_par(1, Vec::new(), false);
    let mut nested = construction
        .import_par(&terminal)
        .expect("import connective terminal");
    let sibling = nested;
    let shell = Connective {
        connective_instance: Some(if conjunction {
            ConnAndBody(ConnectiveBody {
                ps: vec![Par::default(), Par::default()],
            })
        } else {
            ConnOrBody(ConnectiveBody {
                ps: vec![Par::default(), Par::default()],
            })
        }),
    };
    for _ in 0..depth {
        let component = construction
            .connective_component(&shell, &[nested, sibling])
            .expect("plan skewed binary connective");
        nested = construction
            .singleton_component(component, Vec::new(), true)
            .expect("plan connective carrier");
    }
    construction.finish(nested).expect("finish connective plan")
}

fn planned_shallow_first_skewed_connective(depth: usize, conjunction: bool) -> Par {
    let mut construction = ParConstruction::new();
    let terminal = new_gint_par(1, Vec::new(), false);
    let mut nested = construction
        .import_par(&terminal)
        .expect("import connective terminal");
    let sibling = nested;
    let shell = Connective {
        connective_instance: Some(if conjunction {
            ConnAndBody(ConnectiveBody {
                ps: vec![Par::default(), Par::default()],
            })
        } else {
            ConnOrBody(ConnectiveBody {
                ps: vec![Par::default(), Par::default()],
            })
        }),
    };
    for _ in 0..depth {
        let component = construction
            .connective_component(&shell, &[sibling, nested])
            .expect("plan shallow-first skewed binary connective");
        nested = construction
            .singleton_component(component, Vec::new(), true)
            .expect("plan connective carrier");
    }
    construction
        .finish(nested)
        .expect("finish shallow-first connective plan")
}

/// At each level the shallow sibling contains more plan nodes than the entire
/// nested child.  A subtree-size scheduler therefore chooses the shallow side
/// first at every level and retains one ancestor mask per descent.  Its
/// retention pressure remains only three, however, so Ershov ordering chooses
/// the nested side once its pressure reaches four.
fn planned_opposing_width_connective(depth: usize, conjunction: bool, nested_first: bool) -> Par {
    planned_opposing_width_connective_with_terminal(
        depth,
        conjunction,
        nested_first,
        new_gint_par(1, Vec::new(), false),
        1,
    )
}

fn planned_opposing_width_connective_with_terminal(
    depth: usize,
    conjunction: bool,
    nested_first: bool,
    terminal_par: Par,
    terminal_work: usize,
) -> Par {
    assert!(terminal_work > 0);
    let mut construction = ParConstruction::new();
    let terminal = construction
        .import_par(&terminal_par)
        .expect("import opposing-width terminal");
    let mut nested = terminal;
    let mut nested_work = terminal_work;
    let outer_shell = Connective {
        connective_instance: Some(if conjunction {
            ConnAndBody(ConnectiveBody {
                ps: vec![Par::default(), Par::default()],
            })
        } else {
            ConnOrBody(ConnectiveBody {
                ps: vec![Par::default(), Par::default()],
            })
        }),
    };
    for _ in 0..depth {
        let shallow_arity = nested_work / terminal_work + 1;
        let shallow_children = vec![terminal; shallow_arity];
        let shallow_shell = Connective {
            connective_instance: Some(if conjunction {
                ConnAndBody(ConnectiveBody {
                    ps: vec![Par::default(); shallow_arity],
                })
            } else {
                ConnOrBody(ConnectiveBody {
                    ps: vec![Par::default(); shallow_arity],
                })
            }),
        };
        let shallow_component = construction
            .connective_component(&shallow_shell, &shallow_children)
            .expect("plan wider shallow sibling");
        let shallow = construction
            .singleton_component(shallow_component, Vec::new(), true)
            .expect("plan shallow connective carrier");
        let children = if nested_first {
            [nested, shallow]
        } else {
            [shallow, nested]
        };
        let outer_component = construction
            .connective_component(&outer_shell, &children)
            .expect("plan opposing-width outer connective");
        nested = construction
            .singleton_component(outer_component, Vec::new(), true)
            .expect("plan opposing-width carrier");
        nested_work = 1 + nested_work + (1 + shallow_arity * terminal_work);
    }
    construction
        .finish(nested)
        .expect("finish opposing-width connective")
}

fn planned_skewed_failing_or(depth: usize) -> Par {
    let mut construction = ParConstruction::new();
    let mut nested = construction
        .import_par(&new_gint_par(1, Vec::new(), false))
        .expect("import OR success terminal");
    let failure = construction
        .import_par(&new_gint_par(0, Vec::new(), false))
        .expect("import OR refusal terminal");
    let shell = Connective {
        connective_instance: Some(ConnOrBody(ConnectiveBody {
            ps: vec![Par::default(), Par::default()],
        })),
    };
    for _ in 0..depth {
        let component = construction
            .connective_component(&shell, &[failure, nested])
            .expect("plan skewed refusing OR");
        nested = construction
            .singleton_component(component, Vec::new(), true)
            .expect("plan OR carrier");
    }
    construction.finish(nested).expect("finish OR plan")
}

fn planned_binding_conjunction(depth: usize, terminal: i64) -> Par {
    let mut construction = ParConstruction::new();
    let mut nested = construction
        .import_par(&new_gint_par(terminal, Vec::new(), false))
        .expect("import binding-journal terminal");
    let shell = Connective {
        connective_instance: Some(ConnAndBody(ConnectiveBody {
            ps: vec![Par::default(), Par::default()],
        })),
    };
    for level in (0..depth).rev() {
        let level = i32::try_from(level).expect("binding-journal depth fits i32");
        let binding = construction
            .import_par(&new_freevar_par(level, Vec::new()))
            .expect("import binding-journal variable");
        let component = construction
            .connective_component(&shell, &[binding, nested])
            .expect("plan binding-journal conjunction");
        nested = construction
            .singleton_component(component, Vec::new(), true)
            .expect("plan binding-journal carrier");
    }
    construction
        .finish(nested)
        .expect("finish binding-journal plan")
}

#[test]
fn nested_binding_journal_is_linear_and_stack_safe_on_success_and_deep_refusal() {
    const DEPTH: usize = 20_000;
    std::thread::Builder::new()
        .name("spatial-matcher-binding-journal-depth".into())
        .stack_size(256 * 1024)
        .spawn(|| {
            for (terminal, matched) in [(1, true), (0, false)] {
                let pattern = planned_binding_conjunction(DEPTH, terminal);
                let mut context = SpatialMatcherContext::new();
                let (result, stats) = match_par_with_transaction_stats(
                    &mut context,
                    new_gint_par(1, Vec::new(), false),
                    pattern,
                );
                assert_eq!(result.is_some(), matched);
                assert_eq!(stats.writes, DEPTH);
                assert_eq!(stats.peak_entries, stats.writes);
                assert!(stats.checkpoints >= DEPTH);
                assert_eq!(stats.delta_entries, 0);
                if matched {
                    assert_eq!(stats.rollback_entries, 0);
                    assert_eq!(context.free_map.len(), DEPTH);
                } else {
                    assert_eq!(stats.rollback_entries, stats.writes);
                    assert!(context.free_map.is_empty());
                }
            }
        })
        .expect("the fixed-stack binding-journal worker starts")
        .join()
        .expect("the binding journal uses constant native stack");
}

#[test]
fn nested_binding_journal_captures_exact_free_vars_without_terminal_par_decodes() {
    const DEPTH: usize = 64;
    let pattern = planned_binding_conjunction(DEPTH, 1);
    let mut context = SpatialMatcherContext::new();
    let (result, stats) =
        match_par_with_stats(&mut context, new_gint_par(1, Vec::new(), false), pattern);
    assert!(result.is_some());
    assert_eq!(context.free_map.len(), DEPTH);
    assert_eq!(stats.authoritative_exact_free_var_captures, DEPTH);
    assert_eq!(stats.authoritative_terminal_par_decodes, 0);
}

#[test]
fn matcher_drive_unwind_after_a_completed_binding_restores_the_entry_map() {
    let module = module_path!()
        .split_once("::")
        .map(|(_, rest)| rest)
        .expect("module_path! is crate-qualified");
    let child = format!("{module}::matcher_drive_unwind_probe_child");
    assert_unwind_probe(
        &child,
        "matcher-four-writes",
        "FreeMap matcher write failpoint",
    );
}

#[test]
fn matcher_drive_unwind_probe_child() {
    let Some(subject) = unwind_probe_subject() else {
        return;
    };
    assert_eq!(subject, "matcher-four-writes");

    let baseline_value = new_gstring_par("entry baseline".into(), Vec::new(), false);
    let shared = Rc::new(RefCell::new(SpatialMatcherContext::new()));
    shared.borrow_mut().free_map.insert(41, baseline_value);
    let baseline = shared.borrow().free_map.clone();
    let _witness = UnwindRollbackWitness::new(Rc::clone(&shared), baseline, &subject);
    let mut context = shared.borrow_mut();
    let pattern = planned_binding_conjunction(8, 1);

    eprintln!("FREEMAP_UNWIND_REACHED subject={subject}");
    panic_after_writes_for_test(4);
    let _ = context.spatial_match(new_gint_par(1, Vec::new(), false), pattern);
    panic!("matcher unwind probe returned before its configured fourth-write failure");
}

#[test]
fn sole_freevar_binding_collapses_only_projection_equivalent_family_merges() {
    let pattern = new_freevar_par(0, Vec::new());
    let scalar = new_gint_par(7, Vec::new(), false);

    let mixed = scalar.clone().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(models::rhoapi::g_unforgeable::UnfInstance::GDeployIdBody(
            GDeployId { sig: vec![0x2a] },
        )),
    }]);
    let multiplicity = scalar
        .clone()
        .scale_occurrences(16)
        .expect("finite multiplicity scales");
    for target in [Par::default(), scalar.clone(), mixed, multiplicity] {
        let mut recursive = RecursiveContext::new();
        let recursive_result = recursive
            .spatial_match(target.clone(), pattern.clone())
            .is_some();
        let mut context = SpatialMatcherContext::new();
        let (result, stats) =
            match_par_with_transaction_stats(&mut context, target.clone(), pattern.clone());
        assert_eq!(result.is_some(), recursive_result);
        assert_eq!(context.free_map, recursive.free_map);
        assert!(result.is_some());
        assert_eq!(stats.writes, 1);
        assert_eq!(context.free_map.get(&0), Some(&target));
    }

    for target in [
        scalar.clone().with_locally_free(vec![0x01]),
        scalar.clone().with_connective_used(true),
    ] {
        let mut recursive = RecursiveContext::new();
        let recursive_result = recursive
            .spatial_match(target.clone(), pattern.clone())
            .is_some();
        let mut pda = SpatialMatcherContext::new();
        let (pda_result, stats) =
            match_par_with_transaction_stats(&mut pda, target, pattern.clone());
        assert_eq!(pda_result.is_some(), recursive_result);
        assert_eq!(pda.free_map, recursive.free_map);
        assert!(stats.writes > 1);
    }

    let prior = new_gint_par(3, Vec::new(), false);
    let mut recursive = RecursiveContext::new();
    recursive.free_map.insert(0, prior.clone());
    let recursive_result = recursive
        .spatial_match(scalar.clone(), pattern.clone())
        .is_some();
    let mut pda = SpatialMatcherContext::new();
    pda.free_map.insert(0, prior);
    let (pda_result, stats) =
        match_par_with_transaction_stats(&mut pda, scalar.clone(), pattern.clone());
    assert_eq!(pda_result.is_some(), recursive_result);
    assert_eq!(pda.free_map, recursive.free_map);
    assert_eq!(stats.writes, 1);

    let retained_connective = inactive_connective(ConnInt(true));
    let mut recursive = RecursiveContext::new();
    recursive.free_map.insert(0, retained_connective.clone());
    let recursive_result = recursive
        .spatial_match(scalar.clone(), pattern.clone())
        .is_some();
    let mut pda = SpatialMatcherContext::new();
    pda.free_map.insert(0, retained_connective);
    let (pda_result, stats) = match_par_with_transaction_stats(&mut pda, scalar, pattern.clone());
    assert_eq!(pda_result.is_some(), recursive_result);
    assert_eq!(pda.free_map, recursive.free_map);
    assert!(stats.writes > 1);

    let inconsistent_target = Par::default().with_conditionals(vec![If {
        locally_free: vec![0x01],
        ..Default::default()
    }]);
    let mut recursive = RecursiveContext::new();
    let recursive_result = recursive
        .spatial_match(inconsistent_target.clone(), pattern.clone())
        .is_some();
    let mut pda = SpatialMatcherContext::new();
    let (pda_result, stats) =
        match_par_with_transaction_stats(&mut pda, inconsistent_target, pattern);
    assert_eq!(pda_result.is_some(), recursive_result);
    assert!(pda_result.is_none());
    assert_eq!(recursive.free_map.get(&0), Some(&Par::default()));
    assert!(pda.free_map.is_empty());
    assert!(stats.writes > 1);
}

#[test]
fn byte_native_and_resident_candidate_compilers_emit_identical_domains() {
    let targets = (0..8)
        .map(|value| new_gint_par(value, Vec::new(), false))
        .collect::<Vec<_>>();
    for depth in [1, 2, 31, 128] {
        for conjunction in [false, true] {
            let pattern = planned_skewed_connective(depth, conjunction);
            let (encoded, encoded_stats) =
                connective_candidate_positions_with_stats(targets.clone(), pattern.clone(), true);
            let (resident, resident_stats) =
                connective_candidate_positions_with_stats(targets.clone(), pattern, false);
            assert_eq!(encoded, resident);
            assert_eq!(encoded.as_ref(), &[1]);
            assert_eq!(encoded_stats.candidate_encoded_connective_nodes, depth);
            assert_eq!(encoded_stats.candidate_resident_connective_decodes, 0);
            assert_eq!(encoded_stats.candidate_terminal_par_decodes, depth + 1);
            assert_eq!(resident_stats.candidate_encoded_connective_nodes, 0);
            assert_eq!(resident_stats.candidate_resident_connective_decodes, depth);
        }
    }
}

#[test]
fn duplicate_occurrences_remain_one_group_endpoint_until_the_root_expansion() {
    const DEPTH: usize = 64;
    const WIDTH: usize = 128;

    let targets = vec![new_gint_par(1, Vec::new(), false); WIDTH];
    let expected = (0..WIDTH)
        .map(|position| u64::try_from(position).expect("test position fits u64"))
        .collect::<Vec<_>>()
        .into_boxed_slice();

    for conjunction in [false, true] {
        let connective = if conjunction { "and" } else { "or" };
        let pattern = planned_shallow_first_skewed_connective(DEPTH, conjunction);
        let (encoded, encoded_stats) =
            connective_candidate_positions_with_stats(targets.clone(), pattern.clone(), true);
        let (resident, resident_stats) =
            connective_candidate_positions_with_stats(targets.clone(), pattern, false);

        assert_eq!(encoded, expected);
        assert_eq!(resident, expected);
        for (compiler, stats) in [("encoded", encoded_stats), ("resident", resident_stats)] {
            println!(
                "D_GROUP_MASK_DUPLICATES compiler={compiler} connective={connective} \
                 depth={DEPTH} width={WIDTH} \
                 peak_masks={} peak_endpoints={} root_groups={} expanded_positions={}",
                stats.candidate_peak_live_masks,
                stats.candidate_peak_live_mask_endpoints,
                stats.candidate_root_group_endpoints,
                stats.candidate_final_expanded_positions,
            );
            assert_eq!(stats.candidate_root_group_endpoints, 1);
            assert_eq!(stats.candidate_final_expanded_positions, WIDTH);
            assert!(
                stats.candidate_peak_live_masks <= 3,
                "largest-first streaming retained {} logical masks",
                stats.candidate_peak_live_masks
            );
            assert!(
                stats.candidate_peak_live_mask_endpoints <= 3,
                "one duplicate group widened to {} retained endpoints",
                stats.candidate_peak_live_mask_endpoints
            );
        }
    }
}

#[test]
fn retention_pressure_defeats_recursively_opposing_width_and_source_order() {
    const DEPTH: usize = 8;
    let targets = vec![
        new_gint_par(1, Vec::new(), false),
        new_gint_par(2, Vec::new(), false),
    ];
    let expected = vec![0].into_boxed_slice();

    for conjunction in [false, true] {
        for nested_first in [false, true] {
            let pattern = planned_opposing_width_connective(DEPTH, conjunction, nested_first);
            for encoded in [false, true] {
                let (positions, stats) = connective_candidate_positions_with_stats(
                    targets.clone(),
                    pattern.clone(),
                    encoded,
                );
                let (control_positions, control_stats) =
                    connective_candidate_positions_with_node_count_order_stats(
                        targets.clone(),
                        pattern.clone(),
                        encoded,
                    );
                assert_eq!(positions, expected);
                assert_eq!(control_positions, expected);
                println!(
                    "D_GROUP_MASK_OPPOSING_WIDTH compiler={} connective={} nested_first={} \
                     depth={DEPTH} treatment_peak_masks={} treatment_peak_endpoints={} \
                     treatment_accumulators={} node_count_peak_masks={} \
                     node_count_peak_endpoints={} node_count_accumulators={} frame_bytes={}",
                    if encoded { "encoded" } else { "resident" },
                    if conjunction { "and" } else { "or" },
                    nested_first,
                    stats.candidate_peak_live_masks,
                    stats.candidate_peak_live_mask_endpoints,
                    stats.candidate_peak_accumulator_stack,
                    control_stats.candidate_peak_live_masks,
                    control_stats.candidate_peak_live_mask_endpoints,
                    control_stats.candidate_peak_accumulator_stack,
                    stats.candidate_plan_work_frame_bytes,
                );
                assert!(
                    stats.candidate_peak_live_masks <= 4,
                    "pressure scheduling retained {} masks on an opposing-width plan",
                    stats.candidate_peak_live_masks
                );
                assert!(
                    stats.candidate_peak_live_mask_endpoints <= 4,
                    "one matched group widened to {} retained endpoints",
                    stats.candidate_peak_live_mask_endpoints
                );
                assert!(stats.candidate_peak_accumulator_stack <= 2);
                assert!(
                    stats.candidate_plan_work_frame_bytes <= 4 * std::mem::size_of::<usize>(),
                    "the work frame contains more than scalar continuation state: {stats:?}"
                );
                assert!(
                    control_stats.candidate_peak_live_masks >= DEPTH,
                    "the adverse node-count control did not retain a depth slope: {control_stats:?}"
                );
                assert!(
                    control_stats.candidate_peak_live_mask_endpoints >= DEPTH,
                    "the adverse node-count control did not retain endpoint depth: {control_stats:?}"
                );
                assert!(control_stats.candidate_peak_accumulator_stack >= DEPTH);
            }
        }
    }
}

#[test]
fn opposing_depth_width_matrix_retains_distinct_group_domains_by_pressure() {
    for depth in [1usize, 4, 8] {
        for width in [2usize, 8, 32] {
            let terminal = connective(ConnOrBody(ConnectiveBody {
                ps: (0..width)
                    .map(|value| new_gint_par(value as i64, Vec::new(), false))
                    .collect(),
            }));
            let targets = (0..=width)
                .map(|value| new_gint_par(value as i64, Vec::new(), false))
                .collect::<Vec<_>>();
            let expected = (0..width)
                .map(|position| position as u64)
                .collect::<Vec<_>>()
                .into_boxed_slice();
            for conjunction in [false, true] {
                for nested_first in [false, true] {
                    let pattern = planned_opposing_width_connective_with_terminal(
                        depth,
                        conjunction,
                        nested_first,
                        terminal.clone(),
                        width + 1,
                    );
                    for encoded in [false, true] {
                        let (positions, stats) = connective_candidate_positions_with_stats(
                            targets.clone(),
                            pattern.clone(),
                            encoded,
                        );
                        assert_eq!(positions, expected);
                        assert_eq!(stats.candidate_root_group_endpoints, width);
                        assert_eq!(stats.candidate_final_expanded_positions, width);
                        assert!(
                            stats.candidate_peak_live_masks <= 5,
                            "depth={depth} width={width} retained {} masks",
                            stats.candidate_peak_live_masks
                        );
                        assert!(
                            stats.candidate_peak_live_mask_endpoints <= width * 5,
                            "depth={depth} width={width} retained {} endpoints",
                            stats.candidate_peak_live_mask_endpoints
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn adaptive_group_expansion_covers_empty_single_sparse_dense_and_all_lanes() {
    let integer = |value| new_gint_par(value, Vec::new(), false);
    let distinct_targets = (0..128).map(integer).collect::<Vec<_>>();
    let cases = [
        (
            "empty",
            connective(ConnOrBody(ConnectiveBody {
                ps: vec![integer(1_000), integer(1_001)],
            })),
            Vec::new(),
            [1, 0, 0, 0, 0],
        ),
        (
            "sparse",
            connective(ConnOrBody(ConnectiveBody {
                ps: vec![integer(3), integer(97)],
            })),
            vec![3, 97],
            [0, 0, 0, 1, 0],
        ),
        (
            "ordered sparse",
            connective(ConnOrBody(ConnectiveBody {
                ps: (0..96).map(integer).collect(),
            })),
            (0..96).map(|position| position as u64).collect(),
            [0, 0, 0, 1, 0],
        ),
        (
            "all",
            connective(ConnOrBody(ConnectiveBody {
                ps: (0..128).map(integer).collect(),
            })),
            (0..128).map(|position| position as u64).collect(),
            [0, 0, 1, 0, 0],
        ),
    ];
    for (name, pattern, expected, lanes) in cases {
        for encoded in [false, true] {
            let (positions, stats) = connective_candidate_positions_with_stats(
                distinct_targets.clone(),
                pattern.clone(),
                encoded,
            );
            assert_eq!(positions.as_ref(), expected.as_slice(), "{name}");
            assert_eq!(
                [
                    stats.candidate_root_empty_expansions,
                    stats.candidate_root_single_group_expansions,
                    stats.candidate_root_all_expansions,
                    stats.candidate_root_sparse_expansions,
                    stats.candidate_root_dense_expansions,
                ],
                lanes,
                "{name} compiler={}",
                if encoded { "encoded" } else { "resident" }
            );
        }
    }

    let duplicate_targets = vec![integer(1), integer(2), integer(1), integer(1), integer(3)];
    for encoded in [false, true] {
        let (positions, stats) = connective_candidate_positions_with_stats(
            duplicate_targets.clone(),
            connective(ConnOrBody(ConnectiveBody {
                ps: vec![integer(1), integer(99)],
            })),
            encoded,
        );
        assert_eq!(positions.as_ref(), &[0, 2, 3]);
        assert_eq!(stats.candidate_root_single_group_expansions, 1);
    }

    let interleaved_targets = (0..2)
        .flat_map(|_| (0..128).map(integer))
        .collect::<Vec<_>>();
    let dense_pattern = connective(ConnOrBody(ConnectiveBody {
        ps: (0..96).map(integer).collect(),
    }));
    let dense_expected = (0..256)
        .filter(|position| position % 128 < 96)
        .map(|position| position as u64)
        .collect::<Vec<_>>();
    for encoded in [false, true] {
        let (positions, stats) = connective_candidate_positions_with_stats(
            interleaved_targets.clone(),
            dense_pattern.clone(),
            encoded,
        );
        assert_eq!(positions.as_ref(), dense_expected.as_slice());
        assert_eq!(stats.candidate_root_dense_expansions, 1);
    }
}

#[test]
fn sparse_dense_and_adaptive_expansion_are_differentially_order_equivalent() {
    let mut state = 0x8a5c_d789_635d_2dffu64;
    let mut next = || {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        state
    };

    for case in 0..48 {
        let group_count = 4 + usize::try_from(next() % 29).expect("small group count");
        let target_count = group_count * 2 + usize::try_from(next() % 97).expect("small width");
        let target_ids = (0..target_count)
            .map(|_| usize::try_from(next() % group_count as u64).expect("group id fits usize"))
            .collect::<Vec<_>>();
        let mut selected = (0..group_count)
            .map(|_| next() & 3 != 0)
            .collect::<Vec<_>>();
        if selected.iter().filter(|selected| **selected).count() < 2 {
            selected[0] = true;
            selected[1] = true;
        }
        if selected.iter().all(|selected| *selected) {
            selected[group_count - 1] = false;
        }

        let targets = target_ids
            .iter()
            .map(|id| new_gint_par(*id as i64, Vec::new(), false))
            .collect::<Vec<_>>();
        let pattern = connective(ConnOrBody(ConnectiveBody {
            ps: selected
                .iter()
                .enumerate()
                .filter_map(|(id, selected)| {
                    selected.then(|| new_gint_par(id as i64, Vec::new(), false))
                })
                .collect(),
        }));
        let expected = target_ids
            .iter()
            .enumerate()
            .filter_map(|(position, id)| selected[*id].then_some(position as u64))
            .collect::<Vec<_>>();
        let encoded = case % 2 == 0;
        let (adaptive, _) = connective_candidate_positions_with_expansion_mode_stats(
            targets.clone(),
            pattern.clone(),
            encoded,
            CandidateExpansionMode::Adaptive,
        );
        let (sparse, _) = connective_candidate_positions_with_expansion_mode_stats(
            targets.clone(),
            pattern.clone(),
            encoded,
            CandidateExpansionMode::Sparse,
        );
        let (dense, _) = connective_candidate_positions_with_expansion_mode_stats(
            targets,
            pattern,
            encoded,
            CandidateExpansionMode::Dense,
        );
        assert_eq!(
            adaptive.as_ref(),
            expected.as_slice(),
            "adaptive case {case}"
        );
        assert_eq!(sparse, adaptive, "sparse case {case}");
        assert_eq!(dense, adaptive, "dense case {case}");
    }
}

#[test]
fn mixed_duplicate_groups_preserve_ordered_domains_verdict_and_complete_freemap() {
    let integer = |value| new_gint_par(value, Vec::new(), false);
    let targets = vec![
        integer(1),
        integer(1),
        integer(2),
        integer(1),
        integer(2),
        integer(3),
    ];
    let pattern = connective(ConnOrBody(ConnectiveBody {
        ps: vec![integer(1), integer(2)],
    }));
    let expected = vec![0, 1, 2, 3, 4].into_boxed_slice();

    let (encoded, encoded_stats) =
        connective_candidate_positions_with_stats(targets.clone(), pattern.clone(), true);
    let (resident, resident_stats) =
        connective_candidate_positions_with_stats(targets.clone(), pattern.clone(), false);
    assert_eq!(encoded, expected);
    assert_eq!(resident, expected);
    for (compiler, stats) in [("encoded", encoded_stats), ("resident", resident_stats)] {
        println!(
            "D_GROUP_MASK_MIXED compiler={compiler} groups=3 matched_groups={} \
             expanded_positions={} peak_masks={} peak_endpoints={}",
            stats.candidate_root_group_endpoints,
            stats.candidate_final_expanded_positions,
            stats.candidate_peak_live_masks,
            stats.candidate_peak_live_mask_endpoints,
        );
        assert_eq!(stats.candidate_root_group_endpoints, 2);
        assert_eq!(stats.candidate_final_expanded_positions, 5);
    }

    let target = new_eset_par(targets, Vec::new(), false, None, Vec::new(), false);
    let pattern = new_eset_par(
        vec![
            pattern,
            new_freevar_par(0, Vec::new()),
            new_wildcard_par(Vec::new(), true),
            new_wildcard_par(Vec::new(), true),
            new_wildcard_par(Vec::new(), true),
            new_wildcard_par(Vec::new(), true),
        ],
        Vec::new(),
        true,
        None,
        Vec::new(),
        true,
    );
    compare(
        "group-mask expansion preserves mixed-duplicate verdict and complete FreeMap",
        target,
        pattern,
    );
}

#[test]
fn skewed_branching_candidate_plan_compile_evaluate_and_drop_are_stack_safe_at_depth_twenty_thousand(
) {
    const DEPTH: usize = 20_000;
    let handle = std::thread::Builder::new()
        .name("connective-candidate-plan-depth".into())
        .stack_size(256 * 1024)
        .spawn(|| {
            for conjunction in [false, true] {
                let nested = planned_skewed_connective(DEPTH, conjunction);
                let mut context = SpatialMatcherContext::new();
                let (result, stats) = match_par_list_with_stats(
                    &mut context,
                    vec![
                        new_gint_par(0, Vec::new(), false),
                        new_gint_par(1, Vec::new(), false),
                    ],
                    vec![new_gint_par(0, Vec::new(), false), nested],
                );
                assert!(result.is_some());
                assert_eq!(stats.edge_evaluations, 2);
                assert_eq!(stats.candidate_encoded_connective_nodes, DEPTH);
                assert_eq!(stats.candidate_resident_connective_decodes, 0);
                assert_eq!(stats.candidate_terminal_par_decodes, DEPTH + 1);
                assert!(stats.candidate_plan_nodes >= DEPTH * 2 + 1);
                assert_eq!(stats.authoritative_connective_root_copies, 1);
                assert!(stats.authoritative_connective_root_bytes > DEPTH);
                assert_eq!(stats.authoritative_connective_evaluations, DEPTH);
                assert_eq!(stats.authoritative_encoded_connective_nodes, DEPTH);
                assert_eq!(stats.authoritative_resident_connective_fallbacks, 0);
                if conjunction {
                    assert_eq!(stats.authoritative_terminal_par_decodes, 0);
                    assert_eq!(stats.authoritative_exact_scalar_attempts, DEPTH + 1);
                    assert_eq!(stats.authoritative_exact_scalar_equal, DEPTH + 1);
                    assert_eq!(stats.authoritative_exact_scalar_mismatch, 0);
                    assert_eq!(stats.authoritative_and_frames, DEPTH);
                    assert_eq!(stats.authoritative_or_frames, 0);
                    assert!(stats.authoritative_max_frame_depth >= DEPTH * 2);
                } else {
                    assert_eq!(stats.authoritative_terminal_par_decodes, 0);
                    assert_eq!(stats.authoritative_exact_scalar_attempts, 1);
                    assert_eq!(stats.authoritative_exact_scalar_equal, 1);
                    assert_eq!(stats.authoritative_exact_scalar_mismatch, 0);
                    assert_eq!(stats.authoritative_and_frames, 0);
                    assert_eq!(stats.authoritative_or_frames, DEPTH);
                    assert!(stats.authoritative_max_frame_depth >= DEPTH);
                }
            }
        })
        .expect("the tiny-stack candidate compiler worker starts");
    handle
        .join()
        .expect("the candidate compiler and matcher use constant native stack");
}

#[test]
fn encoded_or_refusal_continuations_are_stack_safe_at_depth_twenty_thousand() {
    const DEPTH: usize = 20_000;
    let handle = std::thread::Builder::new()
        .name("encoded-or-refusal-depth".into())
        .stack_size(256 * 1024)
        .spawn(|| {
            let pattern = planned_skewed_failing_or(DEPTH);
            let mut context = SpatialMatcherContext::new();
            let (result, stats) =
                match_par_with_stats(&mut context, new_gint_par(1, Vec::new(), false), pattern);
            assert!(result.is_some());
            assert!(context.free_map.is_empty());
            assert_eq!(stats.authoritative_connective_root_copies, 1);
            assert_eq!(stats.authoritative_connective_evaluations, DEPTH);
            assert_eq!(stats.authoritative_encoded_connective_nodes, DEPTH);
            assert_eq!(stats.authoritative_terminal_par_decodes, 0);
            assert_eq!(stats.authoritative_resident_connective_fallbacks, 0);
            assert_eq!(stats.authoritative_exact_scalar_attempts, DEPTH + 1);
            assert_eq!(stats.authoritative_exact_scalar_equal, 1);
            assert_eq!(stats.authoritative_exact_scalar_mismatch, DEPTH);
            assert_eq!(stats.authoritative_and_frames, 0);
            assert_eq!(stats.authoritative_or_frames, DEPTH * 2);
            assert!(stats.authoritative_max_frame_depth >= DEPTH);
        })
        .expect("the tiny-stack encoded OR worker starts");
    handle
        .join()
        .expect("encoded OR refusal continuations use constant native stack");
}

#[test]
fn production_list_pda_does_not_build_an_unusable_target_index() {
    const WIDTH: usize = 4;
    let targets = (0..WIDTH)
        .map(|value| new_gint_par(value as i64, Vec::new(), false))
        .collect::<Vec<_>>();
    let patterns = (0..WIDTH)
        .map(|_| connective(ConnInt(true)))
        .collect::<Vec<_>>();

    let mut context = SpatialMatcherContext::new();
    let (result, stats) = match_par_list_with_stats(&mut context, targets, patterns);
    assert!(result.is_some());
    assert_eq!(stats.target_index_builds, 0);
    assert_eq!(stats.target_key_encodes, 0);
}

#[test]
fn production_list_pda_reuses_adjacent_duplicate_bag_identity() {
    const WIDTH: usize = 64;
    let element = send_value(
        new_gint_par(7, Vec::new(), false),
        vec![new_gint_par(11, Vec::new(), false)],
        false,
    );

    let mut context = SpatialMatcherContext::new();
    let (result, stats) =
        match_send_list_with_stats(&mut context, vec![element.clone(); WIDTH], vec![
            element;
            WIDTH
        ]);

    assert!(result.is_some());
    assert_eq!(stats.target_index_builds, 1);
    assert_eq!(stats.target_key_encodes, 1);

    // AlwaysEqual and Par's semantic order deliberately ignore
    // `locally_free`, while RMP2 and bincode identity retain it. The reuse
    // guard must therefore use the generated CoW byte-identity proof or these
    // two distinct trie keys collapse.
    let first = send_value(
        new_gint_par(7, Vec::new(), false),
        vec![new_gint_par(11, Vec::new(), false)],
        false,
    );
    let mut second = first.clone();
    second
        .chan
        .as_mut()
        .expect("the Send channel exists")
        .locally_free = vec![1];
    assert_eq!(first, second);
    assert_eq!(first.cmp(&second), std::cmp::Ordering::Equal);

    assert_eq!(send_target_index_shape(vec![first, second]), (2, 2));
}

#[test]
fn send_candidate_query_preserves_matches_across_ignored_cost_and_local_metadata() {
    let mut target = send_value(
        new_gint_par(7, Vec::new(), false),
        vec![new_gint_par(11, Vec::new(), false)],
        false,
    );
    target.cost_region = Some(CostRegion::default());
    target.locally_free = vec![1];
    let pattern = send_value(
        new_gint_par(7, Vec::new(), false),
        vec![new_gint_par(11, Vec::new(), false)],
        true,
    );
    let other = send_value(
        new_gint_par(8, Vec::new(), false),
        vec![new_gint_par(12, Vec::new(), false)],
        false,
    );

    let mut context = SpatialMatcherContext::new();
    let (result, stats) =
        match_send_list_with_stats(&mut context, vec![target, other.clone()], vec![
            pattern, other,
        ]);
    assert!(result.is_some());
    assert_eq!(stats.target_index_builds, 1);
    assert_eq!(stats.edge_evaluations, 2);
}

/// Arbitrary values for the Send fields that the authoritative Send equation
/// never reads: `cost_region` (absent, or populated down to a nested signer)
/// and `locally_free`. `connective_used` keeps the values a normalized pattern
/// and datum carry.
fn ignored_send_metadata() -> impl Strategy<Value = (Option<CostRegion>, Vec<u8>)> {
    let region = (
        any::<u32>(),
        any::<u64>(),
        any::<u64>(),
        any::<u64>(),
        any::<u64>(),
        prop::option::of(prop::collection::vec(any::<u8>(), 0..4)),
        any::<u32>(),
    )
        .prop_map(
            |(version, start_line, start_column, end_line, end_column, signer, ordinal)| {
                CostRegion {
                    version,
                    start_line,
                    start_column,
                    end_line,
                    end_column,
                    signer: signer.map(|ground| CostSignature {
                        atoms: vec![CostSignatureAtom {
                            form: Some(models::rhoapi::cost_signature_atom::Form::Ground(ground)),
                        }],
                    }),
                    ordinal,
                }
            },
        );
    (
        prop::option::of(region),
        prop::collection::vec(any::<u8>(), 0..3),
    )
}

// SpatialMatcher.v::source_wildcard_effect_preserves_capture_state and
// source_query_success_has_unique_emitted_captures, on a Send root. Both Send
// equations (evaluate_send and the recursive oracle) compare only
// `persistent`, `chan` and `data`, and the query compiler skips each ignored
// field as one wildcard. So an ignored field never binds a capture the
// spatial equation observes: the published captures are exactly the levels
// the oracle binds, a repeated compared level still declines without
// publishing a capture, and a linear query accepts every oracle success,
// whatever either side's ignored fields hold.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn send_ignored_fields_never_bind_a_capture_the_spatial_equation_observes(
        chan_level in 0i32..8,
        repeated in any::<bool>(),
        chan in -50i64..50,
        datum in -50i64..50,
        (pattern_region, pattern_locally_free) in ignored_send_metadata(),
        (target_region, target_locally_free) in ignored_send_metadata(),
    ) {
        let data_level = if repeated { chan_level } else { chan_level + 1 };
        let bare_pattern = send_value(
            new_freevar_par(chan_level, Vec::new()),
            vec![new_freevar_par(data_level, Vec::new())],
            true,
        );
        let bare_target = send_value(
            new_gint_par(chan, Vec::new(), false),
            vec![new_gint_par(datum, Vec::new(), false)],
            false,
        );
        let mut pattern = bare_pattern.clone();
        pattern.cost_region = pattern_region;
        pattern.locally_free = pattern_locally_free;
        let mut target = bare_target.clone();
        target.cost_region = target_region;
        target.locally_free = target_locally_free;

        let oracle = |target: &Send, pattern: &Send| {
            let mut context = RecursiveContext::new();
            let verdict = context
                .spatial_match(
                    send_par_of(target.clone()),
                    send_par_of(pattern.clone()),
                )
                .is_some();
            (verdict, context.free_map)
        };
        let (verdict, bindings) = oracle(&target, &pattern);
        let (bare_verdict, bare_bindings) = oracle(&bare_target, &bare_pattern);
        prop_assert_eq!(
            verdict,
            bare_verdict,
            "the spatial verdict observed an ignored Send field"
        );
        prop_assert_eq!(
            &bindings,
            &bare_bindings,
            "the spatial bindings observed an ignored Send field"
        );

        let mut captures = PathMap::new();
        let atom = rho_path_query_cursor_atom(ParCursor::from(&pattern), false, &mut captures);
        let mut bare_captures = PathMap::new();
        let bare_atom =
            rho_path_query_cursor_atom(ParCursor::from(&bare_pattern), false, &mut bare_captures);
        prop_assert_eq!(&atom, &bare_atom, "an ignored Send field changed the compiled query");

        if repeated {
            prop_assert!(atom.is_none(), "a repeated compared level compiled a query");
            prop_assert_eq!(captures.val_count(), 0, "a declined query published captures");
            return Ok(());
        }
        let atom = atom.expect("a linear Send pattern compiles");
        prop_assert!(verdict, "the oracle rejected a linear Send pattern");
        prop_assert_eq!(captures.val_count(), bindings.len());
        for level in bindings.keys() {
            let level = u32::try_from(*level).expect("FreeVar levels are nonnegative");
            prop_assert!(
                captures.contains(level.to_be_bytes()),
                "the query did not publish the oracle-bound level {}",
                level
            );
        }
        let target_structure = encode_structural_sequence(
            [encode_match_path(ParCursor::from(&target)).as_slice()],
        );
        prop_assert!(
            RhoPathQuery::structural_sequence([atom], false).matches_structure(&target_structure),
            "the Send query rejected an oracle success"
        );
    }
}

fn send_par_of(value: Send) -> Par {
    let connective_used = value.connective_used;
    models::par_from_default! {
        sends: vec![value],
        connective_used,
        ..Default::default()
    }
}

#[test]
fn conditional_target_index_retains_distinct_generated_byte_identities() {
    let first = If {
        condition: Some(new_gint_par(7, Vec::new(), false)),
        if_true: Some(new_gint_par(11, Vec::new(), false)),
        if_false: Some(new_gint_par(13, Vec::new(), false)),
        locally_free: Vec::new(),
        connective_used: false,
    };
    let mut second = first.clone();
    second.locally_free = vec![1];

    // Semantic matching deliberately ignores the derived cache, while the
    // generated byte identity used as the PathMap index key must not collapse
    // the two encoded values.
    assert_eq!(first, second, "If AlwaysEqual ignores locally_free");
    assert_eq!(if_target_index_shape(vec![first, second]), (2, 2));
}

#[test]
fn production_list_pda_computes_remainder_as_bag_difference_without_filler_edges() {
    const WIDTH: usize = 128;
    let targets = (0..WIDTH)
        .map(|index| {
            send_value(
                new_gint_par(index as i64, Vec::new(), false),
                vec![new_gint_par((10_000 + index) as i64, Vec::new(), false)],
                false,
            )
        })
        .collect::<Vec<_>>();
    let selected = targets[WIDTH - 1].clone();

    let mut context = SpatialMatcherContext::new();
    let (result, stats) = match_send_list_with_remainder_stats(
        &mut context,
        targets.clone(),
        vec![selected.clone()],
        0,
    );

    assert!(result.is_some());
    assert_eq!(stats.target_index_builds, 1);
    assert_eq!(stats.edge_evaluations, 1);
    assert_eq!(stats.successful_edge_evaluations, 1);
    assert_eq!(
        context.free_map.get(&0),
        Some(&models::par_from_default! {
            sends: targets[..WIDTH - 1].to_vec(),
            ..Default::default()
        })
    );

    compare(
        "Bag-difference remainder preserves the recursive oracle",
        models::par_from_default! {
            sends: targets,
            ..Default::default()
        },
        models::par_from_default! {
            sends: vec![selected],
            exprs: vec![new_freevar_expr(0)],
            connective_used: true,
            ..Default::default()
        },
    );
}

#[test]
fn production_list_pda_bag_residual_is_exhaustively_equivalent_at_small_width() {
    let send_for = |value: usize| {
        send_value(
            new_gint_par(value as i64, Vec::new(), false),
            vec![new_gint_par((100 + value) as i64, Vec::new(), false)],
            false,
        )
    };
    let sequence = |width: usize, bits: usize| {
        (0..width)
            .map(|position| send_for((bits >> position) & 1))
            .collect::<Vec<_>>()
    };

    let mut cases = 0usize;
    for target_width in 0..=4 {
        for target_bits in 0..(1usize << target_width) {
            for pattern_width in 0..=target_width {
                for pattern_bits in 0..(1usize << pattern_width) {
                    let targets = sequence(target_width, target_bits);
                    let patterns = sequence(pattern_width, pattern_bits);
                    compare(
                        &format!(
                            "Bag residual exhaustive target_width={target_width} target_bits={target_bits:#x} pattern_width={pattern_width} pattern_bits={pattern_bits:#x}"
                        ),
                        models::par_from_default! {
                            sends: targets,
                            ..Default::default()
                        },
                        models::par_from_default! {
                            sends: patterns,
                            exprs: vec![new_freevar_expr(0)],
                            connective_used: true,
                            ..Default::default()
                        },
                    );
                    cases += 1;
                }
            }
        }
    }
    assert_eq!(cases, 651, "the bounded model must remain non-vacuous");

    let ground = send_value(
        new_gint_par(7, Vec::new(), false),
        vec![new_gint_par(11, Vec::new(), false)],
        false,
    );
    let open = send_value(new_freevar_par(9, Vec::new()), Vec::new(), true);
    compare(
        "Bag residual retains the predecessor's locally-free refusal",
        models::par_from_default! {
            sends: vec![ground.clone(), open],
            connective_used: true,
            ..Default::default()
        },
        models::par_from_default! {
            sends: vec![ground],
            exprs: vec![new_freevar_expr(0)],
            connective_used: true,
            ..Default::default()
        },
    );
}

#[test]
fn production_list_pda_pathmap_index_prunes_ordered_nested_ac_elements() {
    const WIDTH: usize = 128;
    let targets = (0..WIDTH)
        .map(|index| {
            list(
                vec![
                    new_gint_par(index as i64, Vec::new(), false),
                    new_gint_par((1_000 + index) as i64, Vec::new(), false),
                ],
                false,
            )
        })
        .collect::<Vec<_>>();
    let patterns = (0..WIDTH)
        .rev()
        .map(|index| {
            list(
                vec![
                    new_gint_par(index as i64, Vec::new(), false),
                    new_freevar_par(index as i32, Vec::new()),
                ],
                true,
            )
        })
        .collect::<Vec<_>>();

    let mut context = SpatialMatcherContext::new();
    let (result, stats) =
        match_par_list_with_stats(&mut context, targets.clone(), patterns.clone());
    assert!(result.is_some());
    assert_eq!(stats.target_index_builds, 1);
    assert_eq!(stats.edge_evaluations, WIDTH);
    assert_eq!(stats.successful_edge_evaluations, WIDTH);
    assert_eq!(context.free_map.len(), WIDTH);
    for index in 0..WIDTH {
        assert_eq!(
            context.free_map.get(&(index as i32)),
            Some(&new_gint_par((1_000 + index) as i64, Vec::new(), false))
        );
    }

    compare(
        "PathMap-pruned ordered elements preserve ESet AC semantics",
        new_eset_par(targets, Vec::new(), false, None, Vec::new(), false),
        new_eset_par(patterns, Vec::new(), true, None, Vec::new(), true),
    );
}

#[test]
fn production_list_pda_pathmap_bag_prunes_unordered_send_fields() {
    const WIDTH: usize = 128;
    let targets = (0..WIDTH)
        .map(|index| {
            send_value(
                new_gint_par(index as i64, Vec::new(), false),
                vec![new_gint_par((10_000 + index) as i64, Vec::new(), false)],
                false,
            )
        })
        .collect::<Vec<_>>();
    let patterns = (0..WIDTH)
        .rev()
        .map(|index| {
            send_value(
                new_gint_par(index as i64, Vec::new(), false),
                vec![new_freevar_par(index as i32, Vec::new())],
                true,
            )
        })
        .collect::<Vec<_>>();

    let mut context = SpatialMatcherContext::new();
    let (result, stats) =
        match_send_list_with_stats(&mut context, targets.clone(), patterns.clone());
    assert!(result.is_some());
    assert_eq!(stats.target_index_builds, 1);
    assert_eq!(stats.edge_evaluations, WIDTH);
    assert_eq!(stats.successful_edge_evaluations, WIDTH);
    assert_eq!(context.free_map.len(), WIDTH);
    for index in 0..WIDTH {
        assert_eq!(
            context.free_map.get(&(index as i32)),
            Some(&new_gint_par((10_000 + index) as i64, Vec::new(), false))
        );
    }

    compare(
        "PathMap Bag-pruned sends preserve unordered Par semantics",
        models::par_from_default! {
            sends: targets,
            ..Default::default()
        },
        models::par_from_default! {
            sends: patterns,
            connective_used: true,
            ..Default::default()
        },
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

    let mut original = new_gint_par(7, Vec::new(), false);
    original.locally_free = vec![1];
    let mut mutated = original.clone();
    mutated.locally_free = vec![2];
    assert_eq!(original, mutated, "Par equality ignores root metadata");
    assert_eq!(binding_byte_agreement(&original, &mutated), (false, true));
    assert_eq!(
        binding_byte_agreement(&original, &new_gint_par(8, Vec::new(), false)),
        (false, false),
    );
    let original_map = BTreeMap::from([(41, original)]);
    let mutated_map = BTreeMap::from([(41, mutated)]);
    assert_eq!(original_map, mutated_map);
    compare_with_entry_map(
        "nonempty entry map and fresh capture preserve binding bytes",
        new_gint_par(7, Vec::new(), false),
        new_freevar_par(0, Vec::new()),
        original_map.clone(),
    );
    compare_with_entry_map(
        "nonempty entry map survives root refusal byte-for-byte",
        new_gint_par(7, Vec::new(), false),
        new_gint_par(8, Vec::new(), false),
        original_map,
    );
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

#[test]
fn production_pda_has_no_whole_freemap_snapshot_or_unjournaled_write() {
    let production = include_str!("../../src/rust/interpreter/matcher/spatial_matcher_pda.rs");
    for forbidden in [
        "context.free_map.clone()",
        "context.free_map =",
        "context.free_map.insert",
        "mem::replace(&mut context.free_map",
        "snapshot: FreeMap",
        "single_expr(",
    ] {
        assert!(
            !production.contains(forbidden),
            "production matcher PDA contains forbidden FreeMap operation `{forbidden}`"
        );
    }
    for required in [
        "FreeMapTransaction::new",
        "context.checkpoint()",
        "context.take_delta_and_rollback",
        "context.write(",
        "context.extend(",
        "canonical_exact_free_var_par",
        "sole_free_var_capture_is_admissible",
    ] {
        assert!(
            production.contains(required),
            "production matcher PDA lost journal operation `{required}`"
        );
    }

    let classifier_start = production
        .find("fn sole_free_var_level")
        .expect("the exact sole-FreeVar classifier remains in production");
    let classifier_end = production[classifier_start..]
        .find("struct ConnectivePlan")
        .map(|offset| classifier_start + offset)
        .expect("the classifier region has a stable following declaration");
    let classifier = &production[classifier_start..classifier_end];
    for forbidden in ["single_expr(", "try_visit_decoded"] {
        assert!(
            !classifier.contains(forbidden),
            "sole-FreeVar classification reintroduced recursive endpoint decoding via `{forbidden}`"
        );
    }
    for required in [
        "canonical_exact_free_var_expr_level",
        "concrete_components_locally_free_are_empty",
    ] {
        assert!(
            classifier.contains(required),
            "sole-FreeVar classification lost byte-native proof `{required}`"
        );
    }
}
