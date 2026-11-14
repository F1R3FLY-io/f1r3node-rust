#[cfg(feature = "matcher-decline-counters")]
use models::rhoapi::Send;
use models::rhoapi::{Match, MatchCase, New, Par, Receive, ReceiveBind};
use models::rust::utils::{new_freevar_par, new_gint_par, new_gstring_par};
use rspace_plus_plus::rspace::rho_path::RhoPathQuery;

#[cfg(feature = "matcher-decline-counters")]
use super::projection_rules::declines;
use super::spatial_matcher::{SpatialMatcher, SpatialMatcherContext};
#[cfg(feature = "matcher-decline-counters")]
use super::spatial_matcher_pda::match_dynamic_root_rows_against_themselves;
use super::spatial_matcher_pda::{
    match_ac_query, new_ac_query, receive_ac_query, receive_bind_ac_query,
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
fn dynamic_root_decline_pins_each_of_the_four_roots_to_no_query() {
    let observations = [
        (
            "New",
            accepts(new_root(gint(7)), new_root(free_variable())),
            [
                new_ac_query(new_root(gint(7))).is_none(),
                new_ac_query(new_root(free_variable())).is_none(),
            ],
        ),
        (
            "Receive",
            accepts(receive_root(gint(7)), receive_root(free_variable())),
            [
                receive_ac_query(receive_root(gint(7))).is_none(),
                receive_ac_query(receive_root(free_variable())).is_none(),
            ],
        ),
        (
            "Match",
            accepts(match_root(gint(7)), match_root(free_variable())),
            [
                match_ac_query(match_root(gint(7))).is_none(),
                match_ac_query(match_root(free_variable())).is_none(),
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
#[ignore = "P9 red baseline: the dynamic New root declines its AC query at this commit"]
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
#[ignore = "P9 red baseline: the dynamic Match root declines its AC query at this commit"]
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
