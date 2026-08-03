// See models/src/main/scala/coop/rchain/models/rholang/sorter/ConnectiveSortMatcher.scala
//
// Leg-2 Stage C-2: see `sort_drive` / `sort_combine`.

use super::score_tree::ScoredTerm;
use super::sort_drive::sort_connective;
use super::sortable::Sortable;
use crate::rhoapi::Connective;

pub struct ConnectiveSortMatcher;

impl Sortable<Connective> for ConnectiveSortMatcher {
    fn sort_match(c: &Connective) -> ScoredTerm<Connective> { sort_connective(c) }
}
