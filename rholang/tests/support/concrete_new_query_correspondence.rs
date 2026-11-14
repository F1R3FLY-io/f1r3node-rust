use models::rhoapi::New;
use models::rust::rholang::match_path::encode_match_path;
use models::rust::rholang::match_schema_tables::ParCursor;
use models::rust::utils::new_gint_par;

use super::spatial_matcher::{SpatialMatcher, SpatialMatcherContext};
use super::spatial_matcher_pda::{new_ac_query, new_ac_query_acceptance};

#[test]
fn ground_new_path_query_accepts_across_an_ignored_uri() {
    let target = New {
        bind_count: 1,
        p: Some(new_gint_par(7, Vec::new(), false)),
        uri: vec!["rho:id:target".to_owned()],
        ..New::default()
    };
    let pattern = New {
        uri: vec!["rho:id:pattern".to_owned()],
        ..target.clone()
    };

    let mut direct = SpatialMatcherContext::new();
    assert!(direct
        .spatial_match(target.clone(), pattern.clone())
        .is_some());
    assert_ne!(
        encode_match_path(ParCursor::from(&target)),
        encode_match_path(ParCursor::from(&pattern))
    );

    let query =
        new_ac_query(pattern.clone()).expect("the ground New AC row compiles to a matcher query");
    let retained = query.retained_prefix().len();
    assert!(
        retained > 1,
        "the ground New query retains {retained} bytes, not more than one"
    );
    assert_eq!(
        new_ac_query_acceptance(target, pattern),
        Some(true),
        "the ground New query must accept a target whose matcher-ignored URI differs"
    );
}
