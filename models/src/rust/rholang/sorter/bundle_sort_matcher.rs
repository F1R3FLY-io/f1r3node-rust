// See models/src/main/scala/coop/rchain/models/rholang/sorter/BundleSortMatcher.scala
//
// Leg-2 Stage C-2: see `sort_drive` / `sort_combine`.

use super::score_tree::ScoredTerm;
use super::sort_drive::sort_bundle;
use super::sortable::Sortable;
use crate::rhoapi::Bundle;

pub struct BundleSortMatcher;

impl Sortable<Bundle> for BundleSortMatcher {
    fn sort_match(b: &Bundle) -> ScoredTerm<Bundle> {
        sort_bundle(b)
    }
}
