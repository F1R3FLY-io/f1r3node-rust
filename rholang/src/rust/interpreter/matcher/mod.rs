pub mod exports;
mod free_map_transaction;
pub mod fold_match;
pub mod has_locally_free;
mod lazy_relation;
pub mod list_match;
pub mod r#match;
pub mod match_pars;
pub mod maximum_bipartite_match;
pub mod par_count;
pub mod projection_rules;
pub mod spatial_matcher;
mod spatial_matcher_pda;
pub mod sub_pars;

#[cfg(feature = "spatial-matcher-exact-scalar-differential")]
pub use spatial_matcher_pda::benchmark_match_par_exact_scalar;
#[cfg(feature = "spatial-matcher-candidate-mask-differential")]
pub use spatial_matcher_pda::CandidateMaskBenchmarkFixture;
#[cfg(feature = "spatial-matcher-connective-domain-differential")]
pub use spatial_matcher_pda::{benchmark_match_par_list, benchmark_match_send_list_with_remainder};
#[cfg(feature = "matcher-decline-counters")]
pub use spatial_matcher_pda::{dynamic_root_decline_counts, DynamicRootDeclineCounts};

#[cfg(test)]
#[path = "../../../../tests/support/spatial_matcher_oracle/mod.rs"]
mod recursive_oracle;

#[cfg(test)]
#[path = "../../../../tests/support/spatial_matcher_pda_equivalence.rs"]
mod spatial_matcher_pda_equivalence;

#[cfg(test)]
#[path = "../../../../tests/support/ground_new_query_correspondence.rs"]
mod ground_new_query_correspondence;

#[cfg(test)]
#[path = "../../../../tests/support/dynamic_root_projection_baseline.rs"]
mod dynamic_root_projection_baseline;

#[cfg(test)]
#[path = "../../../../tests/support/concrete_new_query_correspondence.rs"]
mod concrete_new_query_correspondence;
