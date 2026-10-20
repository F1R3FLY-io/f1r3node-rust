// See models/src/main/scala/coop/rchain/models/rholang/sorter/SendSortMatcher.scala
//
// Leg-2 Stage C-2: see `sort_drive` / `sort_combine`.

// Changed by the merge of feature/cost-accounted-rho (DR-131): only the removed
// metered sorter used the names that this edit takes out of these imports.
// use super::par_sort_matcher::ParSortMatcher;
// use super::score_tree::{Score, ScoreAtom, ScoredTerm, Tree};
use super::score_tree::ScoredTerm;
use super::sort_drive::sort_send;
use super::sortable::Sortable;
use crate::rhoapi::Send;

pub struct SendSortMatcher;

impl Sortable<Send> for SendSortMatcher {
    fn sort_match(s: &Send) -> ScoredTerm<Send> { sort_send(s) }
}
