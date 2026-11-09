use super::score_tree::ScoredTerm;
use super::sort_drive::sort_par;
use super::sortable::Sortable;
use crate::rhoapi::Par;

pub struct ParSortMatcher;

impl Sortable<Par> for ParSortMatcher {
    fn sort_match(par: &Par) -> ScoredTerm<Par> { sort_par(par) }
}
