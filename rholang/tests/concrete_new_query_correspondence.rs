use models::rhoapi::{BindPattern, New, Par};
use models::rust::rholang::match_path::encode_match_path;
use models::rust::rholang::match_schema_tables::ParCursor;
use models::rust::utils::new_gint_par;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::matcher::spatial_matcher::{SpatialMatcher, SpatialMatcherContext};
use rspace_plus_plus::rspace::r#match::Match;

#[test]
fn ground_new_path_query_declines_ignored_uri_exact_encoding() {
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
    let query = Matcher.rho_path_query(&bind);
    assert_ne!(pattern_par, target_par);
    assert!(
        query.is_none(),
        "ground New must decline the encoded-exact query when URI is matcher-ignored"
    );
}
