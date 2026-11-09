use super::score_tree::ScoredTerm;
use super::sort_drive::{sort_bind_ref, sort_receive};
use super::sortable::Sortable;
use crate::rhoapi::{Receive, ReceiveBind};

pub struct ReceiveSortMatcher;

impl ReceiveSortMatcher {
    pub fn sort_bind(bind: ReceiveBind) -> ScoredTerm<ReceiveBind> { sort_bind_ref(&bind) }
}

impl Sortable<Receive> for ReceiveSortMatcher {
    fn sort_match(r: &Receive) -> ScoredTerm<Receive> { sort_receive(r) }
}
