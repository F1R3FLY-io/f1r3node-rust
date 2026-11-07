use super::score_tree::ScoredTerm;
use super::sortable::Sortable;
use crate::rhoapi::{Foreach, Par};
use crate::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use crate::rust::rholang::sorter::score_tree::{Score, ScoreAtom, Tree};

pub struct ForeachSortMatcher;

impl Sortable<Foreach> for ForeachSortMatcher {
    fn sort_match(f: &Foreach) -> ScoredTerm<Foreach> {
        let sorted_sources: Vec<ScoredTerm<Par>> = f
            .sources
            .iter()
            .map(|s| ParSortMatcher::sort_match(s))
            .collect();
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
        let sorted_where = f.where_cond.as_ref().map(ParSortMatcher::sort_match);
        let connective_used_score = if f.connective_used { 1 } else { 0 };

        let mut score_children = vec![Tree::<ScoreAtom>::create_leaf_from_i64(f.bind_count as i64)];
        for ss in &sorted_sources {
            score_children.push(ss.score.clone());
        }
        score_children.push(sorted_body.score);
        score_children.push(sorted_yield.score);
        if let Some(sw) = &sorted_where {
            score_children.push(sw.score.clone());
        }
        score_children.push(Tree::<ScoreAtom>::create_leaf_from_i64(
            connective_used_score,
        ));

        ScoredTerm {
            term: Foreach {
                bind_count: f.bind_count,
                sources: sorted_sources.into_iter().map(|s| s.term).collect(),
                body: Some(sorted_body.term),
                yield_name: Some(sorted_yield.term),
                where_cond: sorted_where.map(|s| s.term),
                locally_free: f.locally_free.clone(),
                connective_used: f.connective_used,
            },
            score: Tree::<ScoreAtom>::create_node_from_i32(Score::FOREACH, score_children),
        }
    }
}
