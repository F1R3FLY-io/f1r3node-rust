// See models/src/main/scala/coop/rchain/models/rholang/sorter/NewSortMatcher.scala
//
// Leg-2 Stage C-2: see `sort_drive` / `sort_combine`.

use super::score_tree::ScoredTerm;
use super::sort_drive::sort_new;
use super::sortable::Sortable;
use crate::rhoapi::New;

pub struct NewSortMatcher;

impl Sortable<New> for NewSortMatcher {
    fn sort_match(n: &New) -> ScoredTerm<New> {
        sort_new(n)
    }
}
