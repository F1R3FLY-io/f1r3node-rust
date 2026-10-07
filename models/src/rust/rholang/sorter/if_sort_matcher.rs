// The `if / else` conditional's sort matcher.
//
// Leg-2 Stage C-2: see `sort_drive` / `sort_combine`.

use shared::rust::clone_backing::BackingError;

use super::metered::SorterMeter;
use super::score_tree::ScoredTerm;
use super::sort_drive::sort_if;
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
        let mut scores = meter.vec(4)?;
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
                locally_free: meter.clone(&value.locally_free)?,
                connective_used: value.connective_used,
            },
            score: Tree::<ScoreAtom>::create_node_from_i32_metered(Score::IF, scores, meter)?,
        })
    }
}

impl Sortable<If> for IfSortMatcher {
    fn sort_match(i: &If) -> ScoredTerm<If> { sort_if(i) }
}
