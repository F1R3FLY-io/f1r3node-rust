// See models/src/main/scala/coop/rchain/models/rholang/sorter/ConnectiveSortMatcher.scala
//
// Leg-2 Stage C-2: see `sort_drive` / `sort_combine`.

// Changed by the merge of feature/cost-accounted-rho (DR-131): only the removed
// metered sorter used the names that this edit takes out of these imports.
// use super::par_sort_matcher::ParSortMatcher;
// use super::score_tree::{Score, ScoreAtom, ScoredTerm, Tree};
use super::score_tree::ScoredTerm;
use super::sort_drive::sort_connective;
use super::sortable::Sortable;
// Changed by the merge of feature/cost-accounted-rho (DR-131): only the removed
// metered sorter used the names that this edit takes out of these imports.
// use crate::rhoapi::connective::ConnectiveInstance;
// use crate::rhoapi::{Connective, Par};
use crate::rhoapi::Connective;

pub struct ConnectiveSortMatcher;

impl Sortable<Connective> for ConnectiveSortMatcher {
    fn sort_match(c: &Connective) -> ScoredTerm<Connective> { sort_connective(c) }
}
