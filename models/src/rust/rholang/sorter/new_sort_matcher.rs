// See models/src/main/scala/coop/rchain/models/rholang/sorter/NewSortMatcher.scala
//
// Leg-2 Stage C-2: see `sort_drive` / `sort_combine`.

// Changed by the merge of feature/cost-accounted-rho (DR-131): only the removed
// metered sorter used the names that this edit takes out of these imports.
// use super::par_sort_matcher::ParSortMatcher;
// use super::score_tree::{ScoreAtom, ScoredTerm, Tree};
use super::score_tree::ScoredTerm;
use super::sort_drive::sort_new;
use super::sortable::Sortable;
// Changed by the merge of feature/cost-accounted-rho (DR-131): only the removed
// metered sorter used the names that this edit takes out of these imports.
// use crate::rhoapi::{New, Par};
use crate::rhoapi::New;
// use crate::rust::rholang::sorter::score_tree::Score;

pub struct NewSortMatcher;

impl Sortable<New> for NewSortMatcher {
    fn sort_match(n: &New) -> ScoredTerm<New> { sort_new(n) }
}
