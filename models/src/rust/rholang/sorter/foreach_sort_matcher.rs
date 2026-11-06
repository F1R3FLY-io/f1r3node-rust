use super::score_tree::ScoredTerm;
use super::sortable::Sortable;
use crate::rhoapi::Foreach;
use crate::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use crate::rust::rholang::sorter::score_tree::{Score, ScoreAtom, Tree};

pub struct ForeachSortMatcher;

impl Sortable<Foreach> for ForeachSortMatcher {
    fn sort_match(f: &Foreach) -> ScoredTerm<Foreach> {
        let sorted_source = ParSortMatcher::sort_match(
            f.source
                .as_ref()
                .expect("source field on Foreach was None, should be Some"),
        );
        let sorted_body = ParSortMatcher::sort_match(
            f.body
                .as_ref()
                .expect("body field on Foreach was None, should be Some"),
        );
        let sorted_yield = ParSortMatcher::sort_match(
            f.yield_name
                .as_ref()
                .expect("yield_name field on Foreach was None, should be Some"),
        );
        let connective_used_score = if f.connective_used { 1 } else { 0 };

        ScoredTerm {
            term: Foreach {
                bind_count: f.bind_count,
                source: Some(sorted_source.term),
                body: Some(sorted_body.term),
                yield_name: Some(sorted_yield.term),
                locally_free: f.locally_free.clone(),
                connective_used: f.connective_used,
            },
            score: Tree::<ScoreAtom>::create_node_from_i32(Score::FOREACH, vec![
                Tree::<ScoreAtom>::create_leaf_from_i64(f.bind_count as i64),
                sorted_source.score,
                sorted_body.score,
                sorted_yield.score,
                Tree::<ScoreAtom>::create_leaf_from_i64(connective_used_score),
            ]),
        }
    }
}
