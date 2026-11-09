use super::score_tree::ScoredTerm;
use super::sort_drive::sort_send;
use super::sortable::Sortable;
use crate::rhoapi::Send;

pub struct SendSortMatcher;

impl Sortable<Send> for SendSortMatcher {
    fn sort_match(s: &Send) -> ScoredTerm<Send> { sort_send(s) }
}
