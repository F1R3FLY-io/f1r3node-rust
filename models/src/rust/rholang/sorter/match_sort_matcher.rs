// See models/src/main/scala/coop/rchain/models/rholang/sorter/MatchSortMatcher.scala
//
// Leg-2 Stage C-2: see `sort_drive` / `sort_combine`.

use super::score_tree::ScoredTerm;
use super::sort_drive::sort_match_node;
use super::sortable::Sortable;
use crate::rhoapi::Match;

pub struct MatchSortMatcher;

impl Sortable<Match> for MatchSortMatcher {
    fn sort_match(m: &Match) -> ScoredTerm<Match> {
        sort_match_node(m)
    }
}
