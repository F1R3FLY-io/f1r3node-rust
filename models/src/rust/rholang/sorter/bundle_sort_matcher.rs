// See models/src/main/scala/coop/rchain/models/rholang/sorter/BundleSortMatcher.scala
//
// Leg-2 Stage C-2: see `sort_drive` / `sort_combine`.

// Changed by the merge of feature/cost-accounted-rho (DR-131): only the removed
// metered sorter used the names that this edit takes out of these imports.
// use super::par_sort_matcher::ParSortMatcher;
// use super::score_tree::{Score, ScoreAtom, ScoredTerm, Tree};
use super::score_tree::ScoredTerm;
use super::sort_drive::sort_bundle;
use super::sortable::Sortable;
use crate::rhoapi::Bundle;

pub struct BundleSortMatcher;

impl Sortable<Bundle> for BundleSortMatcher {
    fn sort_match(b: &Bundle) -> ScoredTerm<Bundle> { sort_bundle(b) }
}
