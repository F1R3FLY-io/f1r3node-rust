use super::score_tree::ScoredTerm;
use super::sortable::Sortable;
use crate::rhoapi::If;
use crate::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use crate::rust::rholang::sorter::score_tree::{Score, ScoreAtom, Tree};

pub struct IfSortMatcher;

impl Sortable<If> for IfSortMatcher {
    fn sort_match(i: &If) -> ScoredTerm<If> {
        let sorted_condition = ParSortMatcher::sort_match(
            i.condition
                .as_ref()
                .expect("condition field on If was None, should be Some"),
        );
        let sorted_if_true = ParSortMatcher::sort_match(
            i.if_true
                .as_ref()
                .expect("if_true field on If was None, should be Some"),
        );
        let sorted_if_false = ParSortMatcher::sort_match(
            i.if_false
                .as_ref()
                .expect("if_false field on If was None, should be Some"),
        );
        let connective_used_score = if i.connective_used { 1 } else { 0 };

        ScoredTerm {
            term: If {
                condition: Some(sorted_condition.term),
                if_true: Some(sorted_if_true.term),
                if_false: Some(sorted_if_false.term),
                locally_free: i.locally_free.clone(),
                connective_used: i.connective_used,
            },
            score: Tree::<ScoreAtom>::create_node_from_i32(Score::IF, vec![
                sorted_condition.score,
                sorted_if_true.score,
                sorted_if_false.score,
                Tree::<ScoreAtom>::create_leaf_from_i64(connective_used_score),
            ]),
        }
    }
}
