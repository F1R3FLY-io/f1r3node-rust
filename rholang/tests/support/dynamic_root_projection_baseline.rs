#[allow(dead_code)]
#[path = "planned_deep_fixture.rs"]
mod planned_deep_fixture;

use std::collections::BTreeMap;

#[cfg(feature = "matcher-decline-counters")]
use models::rhoapi::Send;
use models::rhoapi::{Match, MatchCase, New, Par, Receive, ReceiveBind};
use models::rust::utils::{
    new_elist_par, new_freevar_par, new_gbool_par, new_gint_par, new_gstring_par,
};
use proptest::prelude::*;
use rspace_plus_plus::rspace::rho_path::RhoPathQuery;

#[cfg(feature = "matcher-decline-counters")]
use super::projection_rules::declines;
use super::recursive_oracle::spatial_matcher::{
    SpatialMatcher as RecursiveSpatialMatcher, SpatialMatcherContext as RecursiveContext,
};
use super::spatial_matcher::{SpatialMatcher, SpatialMatcherContext};
#[cfg(feature = "matcher-decline-counters")]
use super::spatial_matcher_pda::match_dynamic_root_rows_against_themselves;
use super::spatial_matcher_pda::{
    match_ac_query, match_ac_query_acceptance, new_ac_query, new_ac_query_acceptance,
    receive_ac_query, receive_bind_ac_query,
};
#[cfg(feature = "matcher-decline-counters")]
use super::{dynamic_root_decline_counts, DynamicRootDeclineCounts};

fn gint(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

fn free_variable() -> Par { new_freevar_par(0, Vec::new()) }

fn new_root(observed: Par) -> New {
    New {
        bind_count: 1,
        p: Some(observed),
        ..Default::default()
    }
}

fn receive_bind_root(observed: Par) -> ReceiveBind {
    ReceiveBind {
        patterns: vec![gint(1)],
        source: Some(observed),
        ..Default::default()
    }
}

fn receive_root(observed: Par) -> Receive {
    Receive {
        binds: vec![receive_bind_root(new_gstring_par(
            "dynamic-root".to_owned(),
            Vec::new(),
            false,
        ))],
        connective_used: observed.connective_used(),
        body: Some(observed),
        ..Default::default()
    }
}

fn match_root(observed: Par) -> Match {
    Match {
        target: Some(gint(5)),
        connective_used: observed.connective_used(),
        cases: vec![MatchCase {
            pattern: Some(gint(5)),
            source: Some(observed),
            ..Default::default()
        }],
        ..Default::default()
    }
}

fn accepts<T>(target: T, pattern: T) -> Option<()>
where SpatialMatcherContext: SpatialMatcher<T, T> {
    SpatialMatcherContext::new().spatial_match(target, pattern)
}

#[test]
fn dynamic_root_decline_pins_receive_and_receive_bind_to_no_query() {
    let observations = [
        (
            "Receive",
            accepts(receive_root(gint(7)), receive_root(free_variable())),
            [
                receive_ac_query(receive_root(gint(7))).is_none(),
                receive_ac_query(receive_root(free_variable())).is_none(),
            ],
        ),
        (
            "ReceiveBind",
            accepts(
                receive_bind_root(gint(7)),
                receive_bind_root(free_variable()),
            ),
            [
                receive_bind_ac_query(receive_bind_root(gint(7))).is_none(),
                receive_bind_ac_query(receive_bind_root(free_variable())).is_none(),
            ],
        ),
    ];
    for (root, verdict, declined) in observations {
        assert_eq!(
            verdict,
            Some(()),
            "{root}: the production PDA accepts the dynamic pattern"
        );
        assert_eq!(
            declined,
            [true, true],
            "{root}: the ground and the dynamic AC row compile no matcher query"
        );
    }
}

#[cfg(feature = "matcher-decline-counters")]
fn send_on(datum: i64) -> Send {
    Send {
        chan: Some(new_gstring_par(
            "dynamic-root".to_owned(),
            Vec::new(),
            false,
        )),
        data: vec![gint(datum)],
        ..Default::default()
    }
}

#[cfg(feature = "matcher-decline-counters")]
fn counts_since(
    before: DynamicRootDeclineCounts,
    after: DynamicRootDeclineCounts,
) -> DynamicRootDeclineCounts {
    DynamicRootDeclineCounts {
        rows_compiled: after.rows_compiled - before.rows_compiled,
        rows_declined: after.rows_declined - before.rows_declined,
        declined_new: after.declined_new - before.declined_new,
        declined_receive: after.declined_receive - before.declined_receive,
        declined_match: after.declined_match - before.declined_match,
        declined_receive_bind: after.declined_receive_bind - before.declined_receive_bind,
    }
}

#[cfg(feature = "matcher-decline-counters")]
#[test]
fn dynamic_root_decline_counters_count_every_row_and_each_declined_root() {
    let before = dynamic_root_decline_counts();
    let verdicts = match_dynamic_root_rows_against_themselves(
        vec![new_root(gint(1)), new_root(gint(2))],
        vec![receive_root(gint(1)), receive_root(gint(2))],
        vec![match_root(gint(1)), match_root(gint(2))],
        vec![receive_bind_root(gint(1)), receive_bind_root(gint(2))],
        vec![send_on(1), send_on(2)],
    );
    let counted = counts_since(before, dynamic_root_decline_counts());
    assert_eq!(
        verdicts,
        [Some(()); 5],
        "every two-row AC list matches itself"
    );
    let [declined_new, declined_receive, declined_match, declined_receive_bind] =
        ["New", "Receive", "Match", "ReceiveBind"].map(|root| 2 * u64::from(declines(root)));
    assert_eq!(counted, DynamicRootDeclineCounts {
        rows_compiled: 10,
        rows_declined: declined_new + declined_receive + declined_match + declined_receive_bind,
        declined_new,
        declined_receive,
        declined_match,
        declined_receive_bind,
    });
    assert_eq!(
        DynamicRootDeclineCounts {
            rows_compiled: 10,
            rows_declined: 8,
            declined_new: 2,
            declined_receive: 2,
            declined_match: 2,
            declined_receive_bind: 2,
        }
        .to_string(),
        "matcher-decline-counters rows_compiled=10 rows_declined=8 declined_new=2 \
         declined_receive=2 declined_match=2 declined_receive_bind=2"
    );
}

fn assert_retained_prefixes_are_longer_than_one_byte(
    root: &str,
    queries: [Option<RhoPathQuery>; 2],
) {
    for (row, query) in ["ground", "dynamic"].into_iter().zip(queries) {
        let Some(query) = query else {
            panic!("{root}: the {row} AC row compiles no query, so it retains no prefix");
        };
        let retained = query.retained_prefix().len();
        assert!(
            retained > 1,
            "{root}: the {row} AC row retains {retained} bytes, not more than one"
        );
    }
}

#[test]
fn dynamic_root_retained_prefix_is_longer_than_one_byte_for_new() {
    assert_retained_prefixes_are_longer_than_one_byte("New", [
        new_ac_query(new_root(gint(7))),
        new_ac_query(new_root(free_variable())),
    ]);
}

#[test]
#[ignore = "P9 red baseline: the dynamic Receive root declines its AC query at this commit"]
fn dynamic_root_retained_prefix_is_longer_than_one_byte_for_receive() {
    assert_retained_prefixes_are_longer_than_one_byte("Receive", [
        receive_ac_query(receive_root(gint(7))),
        receive_ac_query(receive_root(free_variable())),
    ]);
}

#[test]
fn dynamic_root_retained_prefix_is_longer_than_one_byte_for_match() {
    assert_retained_prefixes_are_longer_than_one_byte("Match", [
        match_ac_query(match_root(gint(7))),
        match_ac_query(match_root(free_variable())),
    ]);
}

#[test]
#[ignore = "P9 red baseline: the dynamic ReceiveBind root declines its AC query at this commit"]
fn dynamic_root_retained_prefix_is_longer_than_one_byte_for_receive_bind() {
    assert_retained_prefixes_are_longer_than_one_byte("ReceiveBind", [
        receive_bind_ac_query(receive_bind_root(gint(7))),
        receive_bind_ac_query(receive_bind_root(free_variable())),
    ]);
}

type MatchOutcome = (Option<()>, BTreeMap<i32, Par>);

fn direct_match<T>(target: T, pattern: T) -> MatchOutcome
where SpatialMatcherContext: SpatialMatcher<T, T> {
    let mut context = SpatialMatcherContext::new();
    let verdict = context.spatial_match(target, pattern);
    (verdict, context.free_map)
}

fn oracle_match<T>(target: T, pattern: T) -> MatchOutcome
where RecursiveContext: RecursiveSpatialMatcher<T, T> {
    let mut context = RecursiveContext::new();
    let verdict = context.spatial_match(target, pattern);
    (verdict, context.free_map)
}

fn oracle_success_is_accepted(
    root: &str,
    oracle: MatchOutcome,
    pda: MatchOutcome,
    acceptance: Option<bool>,
) -> Result<(), TestCaseError> {
    prop_assert!(
        oracle.0.is_some(),
        "{root}: the recursive oracle must accept a pattern that differs only in ignored fields"
    );
    prop_assert_eq!(pda, oracle, "{}: the PDA and the oracle disagree", root);
    prop_assert_eq!(
        acceptance,
        Some(true),
        "{}: the AC row query rejected an oracle success across its ignored fields",
        root
    );
    Ok(())
}

#[derive(Clone, Debug)]
struct IgnoredNewFields {
    uri: Vec<String>,
    injections: BTreeMap<String, Par>,
    locally_free: Vec<u8>,
}

fn ignored_new_fields() -> impl Strategy<Value = IgnoredNewFields> {
    (
        prop::collection::vec(any::<u16>(), 0..3),
        prop::collection::btree_map(any::<u16>(), any::<i16>(), 0..3),
        prop::collection::vec(any::<u8>(), 0..3),
    )
        .prop_map(|(uris, injections, locally_free)| IgnoredNewFields {
            uri: uris
                .into_iter()
                .map(|seed| format!("rho:id:{seed}"))
                .collect(),
            injections: injections
                .into_iter()
                .map(|(name, value)| (format!("rho:io:{name}"), gint(i64::from(value))))
                .collect(),
            locally_free,
        })
}

#[derive(Clone, Debug)]
struct IgnoredMatchFields {
    free_count: i32,
    guard: Option<Par>,
    locally_free: Vec<u8>,
}

fn ignored_match_fields() -> impl Strategy<Value = IgnoredMatchFields> {
    (
        0_i32..4,
        prop::option::of(any::<bool>()),
        prop::collection::vec(any::<u8>(), 0..3),
    )
        .prop_map(|(free_count, guard, locally_free)| IgnoredMatchFields {
            free_count,
            guard: guard.map(|flag| new_gbool_par(flag, Vec::new(), false)),
            locally_free,
        })
}

fn singleton_list(element: Par) -> Par {
    new_elist_par(vec![element], Vec::new(), false, None, Vec::new(), false)
}

fn case_pattern(kind: u8, value: i16) -> Par {
    match kind {
        0 => gint(i64::from(value)),
        1 => free_variable(),
        _ => singleton_list(gint(i64::from(value))),
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: None,
        ..ProptestConfig::with_cases(64)
    })]

    #[test]
    fn dynamic_root_query_accepts_oracle_successes_across_ignored_fields_for_new(
        bind_count in 0_i32..=4,
        body in any::<i16>(),
        body_is_hole in any::<bool>(),
        target_fields in ignored_new_fields(),
        pattern_fields in ignored_new_fields(),
    ) {
        let target = New {
            bind_count,
            p: Some(gint(i64::from(body))),
            uri: target_fields.uri,
            injections: target_fields.injections,
            locally_free: target_fields.locally_free,
        };
        let pattern = New {
            bind_count,
            p: Some(if body_is_hole {
                free_variable()
            } else {
                gint(i64::from(body))
            }),
            uri: pattern_fields.uri,
            injections: pattern_fields.injections,
            locally_free: pattern_fields.locally_free,
        };
        let acceptance = new_ac_query_acceptance(target.clone(), pattern.clone());
        let oracle = oracle_match(target.clone(), pattern.clone());
        oracle_success_is_accepted("New", oracle, direct_match(target, pattern), acceptance)?;
    }

    #[test]
    fn dynamic_root_query_accepts_oracle_successes_across_ignored_fields_for_match(
        scrutinee in any::<i16>(),
        scrutinee_is_hole in any::<bool>(),
        case_kind in 0_u8..3,
        case_value in any::<i16>(),
        source in any::<i16>(),
        source_is_hole in any::<bool>(),
        target_fields in ignored_match_fields(),
        pattern_fields in ignored_match_fields(),
    ) {
        let case = case_pattern(case_kind, case_value);
        let target = Match {
            target: Some(gint(i64::from(scrutinee))),
            cases: vec![MatchCase {
                pattern: Some(case.clone()),
                source: Some(gint(i64::from(source))),
                free_count: target_fields.free_count,
                guard: target_fields.guard,
            }],
            locally_free: target_fields.locally_free,
            connective_used: false,
        };
        let source_level = i32::from(scrutinee_is_hole);
        let pattern = Match {
            target: Some(if scrutinee_is_hole {
                free_variable()
            } else {
                gint(i64::from(scrutinee))
            }),
            cases: vec![MatchCase {
                pattern: Some(case),
                source: Some(if source_is_hole {
                    new_freevar_par(source_level, Vec::new())
                } else {
                    gint(i64::from(source))
                }),
                free_count: pattern_fields.free_count,
                guard: pattern_fields.guard,
            }],
            locally_free: pattern_fields.locally_free,
            connective_used: scrutinee_is_hole || source_is_hole,
        };
        let acceptance = match_ac_query_acceptance(target.clone(), pattern.clone());
        let oracle = oracle_match(target.clone(), pattern.clone());
        oracle_success_is_accepted("Match", oracle, direct_match(target, pattern), acceptance)?;
    }
}

const DEEP_ROOT_DEPTH: usize = 4_096;

fn on_a_one_mebibyte_thread(name: &str, check: fn()) {
    std::thread::Builder::new()
        .name(name.to_owned())
        .stack_size(1024 * 1024)
        .spawn(check)
        .expect("spawn the 1 MiB depth-4,096 thread")
        .join()
        .expect("the depth-4,096 thread completes on its 1 MiB stack");
}

fn deep_target_body() -> Par {
    Par::wrap_unary_elist(DEEP_ROOT_DEPTH, gint(99))
        .expect("canonical unary-list datum is representable")
}

fn deep_pattern_body() -> Par { planned_deep_fixture::nested_list_pattern(DEEP_ROOT_DEPTH) }

fn assert_deep_root_compiles_and_matches(
    root: &str,
    query: Option<RhoPathQuery>,
    acceptance: Option<bool>,
    matched: MatchOutcome,
) {
    let Some(query) = query else {
        panic!("{root}: the depth-4,096 pattern compiles no query");
    };
    let retained = query.retained_prefix().len();
    assert!(
        retained > 1,
        "{root}: the depth-4,096 query retains {retained} bytes, not more than one"
    );
    assert_eq!(
        acceptance,
        Some(true),
        "{root}: the depth-4,096 query rejects its matching target"
    );
    assert_eq!(
        matched,
        (Some(()), BTreeMap::from([(0, gint(99))])),
        "{root}: the production PDA binds the depth-4,096 terminal"
    );
}

#[test]
fn dynamic_root_depth_4096_pattern_compiles_and_matches_on_a_one_mebibyte_thread_for_new() {
    on_a_one_mebibyte_thread("depth-4096-new-root", || {
        let target = new_root(deep_target_body());
        let pattern = new_root(deep_pattern_body());
        assert_deep_root_compiles_and_matches(
            "New",
            new_ac_query(pattern.clone()),
            new_ac_query_acceptance(target.clone(), pattern.clone()),
            direct_match(target, pattern),
        );
    });
}

#[test]
fn dynamic_root_depth_4096_pattern_compiles_and_matches_on_a_one_mebibyte_thread_for_match() {
    on_a_one_mebibyte_thread("depth-4096-match-root", || {
        let target = match_root(deep_target_body());
        let pattern = match_root(deep_pattern_body());
        assert_deep_root_compiles_and_matches(
            "Match",
            match_ac_query(pattern.clone()),
            match_ac_query_acceptance(target.clone(), pattern.clone()),
            direct_match(target, pattern),
        );
    });
}
