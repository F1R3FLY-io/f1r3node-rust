// See models/src/main/scala/coop/rchain/models/rholang/sorter/MatchSortMatcher.scala
//
// Leg-2 Stage C-2: see `sort_drive` / `sort_combine`.

use super::score_tree::ScoredTerm;
use super::sort_drive::sort_match_node;
use super::sortable::Sortable;
// Changed by the merge of feature/cost-accounted-rho (DR-131): only the removed
// metered sorter used the names that this edit takes out of these imports.
// use crate::rhoapi::{Match, MatchCase, Par};
use crate::rhoapi::Match;
// use crate::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
// use crate::rust::rholang::sorter::score_tree::{Score, ScoreAtom, Tree};

pub struct MatchSortMatcher;

impl Sortable<Match> for MatchSortMatcher {
    fn sort_match(m: &Match) -> ScoredTerm<Match> { sort_match_node(m) }
}
