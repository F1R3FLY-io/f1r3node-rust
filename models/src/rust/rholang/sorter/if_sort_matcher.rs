// The `if / else` conditional's sort matcher.
//
// Leg-2 Stage C-2: see `sort_drive` / `sort_combine`.

use super::score_tree::ScoredTerm;
use super::sort_drive::sort_if;
use super::sortable::Sortable;
use crate::rhoapi::If;
// Changed by the merge of feature/cost-accounted-rho (DR-131): only the removed
// metered sorter used the names that this edit takes out of these imports.
// use crate::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
// use crate::rust::rholang::sorter::score_tree::{Score, ScoreAtom, Tree};

pub struct IfSortMatcher;

impl Sortable<If> for IfSortMatcher {
    fn sort_match(i: &If) -> ScoredTerm<If> { sort_if(i) }
}
