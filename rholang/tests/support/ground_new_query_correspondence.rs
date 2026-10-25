use std::collections::BTreeMap;

use models::rhoapi::New;
use models::rust::rholang::match_path::encode_match_path;
use models::rust::rholang::match_schema_tables::ParCursor;
use models::rust::utils::new_gint_par;
use proptest::prelude::*;
use rspace_plus_plus::rspace::rho_path::{
    encode_structural_sequence, RhoPathQuery, RhoPathQueryAtom,
};

use super::recursive_oracle::spatial_matcher::{
    SpatialMatcher as RecursiveSpatialMatcher, SpatialMatcherContext as RecursiveContext,
};
use super::spatial_matcher::{SpatialMatcher, SpatialMatcherContext};
use super::spatial_matcher_pda::new_ac_query_acceptance;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn generated_ground_new_query_never_rejects_oracle_success(
        bind_count in 0_i32..=8,
        body in any::<i16>(),
        uri_seed in any::<u16>(),
    ) {
        let target = New {
            bind_count,
            p: Some(new_gint_par(i64::from(body), Vec::new(), false)),
            uri: vec![format!("rho:id:target:{uri_seed}")],
            injections: BTreeMap::new(),
            locally_free: Vec::new(),
        };
        for same_uri in [true, false] {
            for different_injections in [false, true] {
                for different_locally_free in [false, true] {
                    let mut pattern = target.clone();
                    if !same_uri {
                        pattern.uri = vec![format!("rho:id:pattern:{uri_seed}")];
                    }
                    if different_injections {
                        pattern.injections.insert(
                            "ignored-injection".to_owned(),
                            new_gint_par(i64::from(body) + 1, Vec::new(), false),
                        );
                    }
                    if different_locally_free {
                        pattern.locally_free.push(1);
                    }

                    let mut recursive = RecursiveContext::new();
                    let recursive_verdict = recursive
                        .spatial_match(target.clone(), pattern.clone())
                        .is_some();
                    prop_assert!(recursive_verdict);

                    let candidate = new_ac_query_acceptance(target.clone(), pattern.clone());
                    prop_assert_ne!(candidate, Some(false));

                    let mut pda = SpatialMatcherContext::new();
                    let pda_verdict = pda.spatial_match(target.clone(), pattern.clone()).is_some();
                    prop_assert_eq!(pda_verdict, recursive_verdict);
                    prop_assert_eq!(pda.free_map, recursive.free_map);

                    if !same_uri {
                        let target_bytes = encode_match_path(ParCursor::from(&target));
                        let pattern_bytes = encode_match_path(ParCursor::from(&pattern));
                        let old_exact_query = RhoPathQuery::structural_sequence(
                            [RhoPathQueryAtom::Exact(pattern_bytes)],
                            false,
                        );
                        let target_structure = encode_structural_sequence([target_bytes.as_slice()]);
                        prop_assert!(
                            !old_exact_query.matches_structure(&target_structure),
                            "the removed encoded-exact lane must reject this oracle-accepted URI variant"
                        );
                    }
                }
            }
        }
    }
}
