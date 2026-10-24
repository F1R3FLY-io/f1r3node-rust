use super::score_tree::ScoredTerm;
use super::sort_drive::sort_connective;
use super::sortable::Sortable;
use crate::rhoapi::Connective;

pub struct ConnectiveSortMatcher;

impl Sortable<Connective> for ConnectiveSortMatcher {
    fn sort_match(c: &Connective) -> ScoredTerm<Connective> { sort_connective(c) }
}
