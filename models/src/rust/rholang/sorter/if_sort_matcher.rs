use shared::rust::clone_backing::BackingError;

use super::metered::SorterMeter;
use super::score_tree::ScoredTerm;
use super::sortable::Sortable;
use crate::rhoapi::If;
use crate::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use crate::rust::rholang::sorter::score_tree::{Score, ScoreAtom, Tree};

pub struct IfSortMatcher;

impl IfSortMatcher {
    pub fn sort_match_metered(
        value: &If,
        meter: &SorterMeter<'_>,
    ) -> Result<ScoredTerm<If>, BackingError> {
        let condition = ParSortMatcher::sort_match_metered(
            value.condition.as_ref().ok_or(BackingError::Rejected)?,
            meter,
        )?;
        let if_true = ParSortMatcher::sort_match_metered(
            value.if_true.as_ref().ok_or(BackingError::Rejected)?,
            meter,
        )?;
        let if_false = ParSortMatcher::sort_match_metered(
            value.if_false.as_ref().ok_or(BackingError::Rejected)?,
            meter,
        )?;
        // Changed by D-E4 (DR-111): Rule S, one read of each slot.
        // let mut scores = meter.vec(4)?;
        let mut scores = meter.score_vec(4)?;
        scores.push(condition.score);
        scores.push(if_true.score);
        scores.push(if_false.score);
        scores.push(Tree::<ScoreAtom>::create_leaf_from_i64(
            value.connective_used as i64,
        ));
        Ok(ScoredTerm {
            term: If {
                condition: Some(condition.term),
                if_true: Some(if_true.term),
                if_false: Some(if_false.term),
                // Changed by D-O1 (DR-111): block accounting.
                // locally_free: meter.clone(&value.locally_free)?,
                locally_free: meter.clone_blocks(&value.locally_free)?,
                connective_used: value.connective_used,
            },
            score: Tree::<ScoreAtom>::create_node_from_i32_metered(Score::IF, scores, meter)?,
        })
    }
}

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
