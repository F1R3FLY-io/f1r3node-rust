pub mod exports;
pub mod fold_match;
pub mod has_locally_free;
pub mod list_match;
pub mod r#match;
pub mod match_pars;
pub mod maximum_bipartite_match;
pub mod par_count;
pub mod spatial_matcher;
mod spatial_matcher_pda;
pub mod sub_pars;

#[cfg(test)]
#[path = "../../../../tests/support/spatial_matcher_oracle/mod.rs"]
mod recursive_oracle;

#[cfg(test)]
#[path = "../../../../tests/support/spatial_matcher_pda_equivalence.rs"]
mod spatial_matcher_pda_equivalence;
