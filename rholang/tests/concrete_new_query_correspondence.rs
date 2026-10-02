use models::rhoapi::{BindPattern, ListParWithRandom, New, Par};
use models::rust::rholang::match_path::encode_match_path;
use models::rust::rholang::match_schema_tables::ParCursor;
use models::rust::utils::new_gint_par;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::matcher::spatial_matcher::{SpatialMatcher, SpatialMatcherContext};
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::rho_path::RhoPathEncode;

#[test]
fn ground_new_query_retains_spatial_matches_across_ignored_uri() {
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

    let target_par = Par::default().with_news(vec![target]);
    let pattern_par = Par::default().with_news(vec![pattern]);
    let bind = BindPattern {
        patterns: vec![pattern_par.clone()],
        remainder: None,
        free_count: 0,
    };
    let datum = ListParWithRandom {
        pars: vec![target_par.clone()],
        ..ListParWithRandom::default()
    };
    let query = Matcher
        .rho_path_query(&bind)
        .expect("ground New compiles a query");
    assert_ne!(pattern_par, target_par);
    assert!(
        query.matches_structure(&datum.rho_path_structure()),
        "generated query must retain a candidate accepted by the New spatial equation"
    );
    let mut root = SpatialMatcherContext::new();
    assert!(root.spatial_match_result(target_par, pattern_par).is_some());
}
