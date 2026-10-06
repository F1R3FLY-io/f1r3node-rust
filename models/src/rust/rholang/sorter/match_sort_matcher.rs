// See models/src/main/scala/coop/rchain/models/rholang/sorter/MatchSortMatcher.scala
//
// Leg-2 Stage C-2: see `sort_drive` / `sort_combine`.

use shared::rust::clone_backing::BackingError;

use super::metered::SorterMeter;
use super::score_tree::ScoredTerm;
use super::sort_drive::sort_match_node;
use super::sortable::Sortable;
use crate::rhoapi::Match;

pub struct MatchSortMatcher;

impl MatchSortMatcher {
    pub fn sort_match_metered(
        value: &Match,
        meter: &SorterMeter<'_>,
    ) -> Result<ScoredTerm<Match>, BackingError> {
        let _depth = meter.enter()?;
        let target = ParSortMatcher::sort_match_metered(
            value.target.as_ref().ok_or(BackingError::Rejected)?,
            meter,
        )?;
        let mut cases = meter.vec(value.cases.len())?;
        let mut scores = meter.vec(
            value
                .cases
                .len()
                .checked_add(2)
                .ok_or(BackingError::Overflow)?,
        )?;
        scores.push(target.score);
        for case in &value.cases {
            let scored = Self::sort_case_metered(case, meter)?;
            cases.push(scored.term);
            scores.push(scored.score);
        }
        scores.push(Tree::<ScoreAtom>::create_leaf_from_i64(
            value.connective_used as i64,
        ));
        Ok(ScoredTerm {
            term: Match {
                target: Some(target.term),
                cases,
                locally_free: meter.clone(&value.locally_free)?,
                connective_used: value.connective_used,
            },
            score: Tree::<ScoreAtom>::create_node_from_i32_metered(Score::MATCH, scores, meter)?,
        })
    }

    fn sort_case_metered(
        value: &MatchCase,
        meter: &SorterMeter<'_>,
    ) -> Result<ScoredTerm<MatchCase>, BackingError> {
        let _depth = meter.enter()?;
        let pattern = ParSortMatcher::sort_match_metered(
            value.pattern.as_ref().ok_or(BackingError::Rejected)?,
            meter,
        )?;
        let body = ParSortMatcher::sort_match_metered(
            value.source.as_ref().ok_or(BackingError::Rejected)?,
            meter,
        )?;
        let empty = Par::default();
        let guard =
            ParSortMatcher::sort_match_metered(value.guard.as_ref().unwrap_or(&empty), meter)?;
        let guard_term = value
            .guard
            .as_ref()
            .filter(|par| *par != &empty)
            .map(|_| guard.term);
        let mut scores = meter.vec(4)?;
        scores.push(pattern.score);
        scores.push(body.score);
        scores.push(Tree::<ScoreAtom>::create_leaf_from_i64(i64::from(
            value.free_count,
        )));
        scores.push(guard.score);
        Ok(ScoredTerm {
            term: MatchCase {
                pattern: Some(pattern.term),
                source: Some(body.term),
                free_count: value.free_count,
                guard: guard_term,
            },
            score: Tree::Node(scores),
        })
    }
}

impl Sortable<Match> for MatchSortMatcher {
    fn sort_match(m: &Match) -> ScoredTerm<Match> { sort_match_node(m) }
}
