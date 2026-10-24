use super::score_tree::ScoredTerm;
use super::sort_drive::sort_if;
use super::sortable::Sortable;
use crate::rhoapi::If;

pub struct IfSortMatcher;

impl Sortable<If> for IfSortMatcher {
    fn sort_match(i: &If) -> ScoredTerm<If> { sort_if(i) }
}
