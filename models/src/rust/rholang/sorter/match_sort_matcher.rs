// See models/src/main/scala/coop/rchain/models/rholang/sorter/MatchSortMatcher.scala

use shared::rust::clone_backing::BackingError;

use super::metered::SorterMeter;
use super::score_tree::ScoredTerm;
use super::sortable::Sortable;
use crate::rhoapi::{Match, MatchCase, Par};
use crate::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use crate::rust::rholang::sorter::score_tree::{Score, ScoreAtom, Tree};

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
    fn sort_match(m: &Match) -> ScoredTerm<Match> {
        fn sort_case(match_case: &MatchCase) -> ScoredTerm<MatchCase> {
            let sorted_pattern = ParSortMatcher::sort_match(
                match_case
                    .pattern
                    .as_ref()
                    .expect("pattern field on MatchCase was None, should be Some"),
            );
            let sorted_body = ParSortMatcher::sort_match(
                match_case
                    .source
                    .as_ref()
                    .expect("source field on MatchCase was None, should be Some"),
            );
            let free_count_score =
                Tree::<ScoreAtom>::create_leaf_from_i64(match_case.free_count as i64);

            // The optional `where` guard: if absent, sort the empty Par
            // and contribute a stable but non-empty score node so that
            // un-guarded cases still hash deterministically. Collapse
            // `Some(empty Par)` to `None` so the wire term is the same
            // either way — the four guard sites already treat empty as
            // absent, so we mustn't leak the distinction here.
            let guard_par = match_case.guard.clone().unwrap_or_default();
            let sorted_guard = ParSortMatcher::sort_match(&guard_par);
            let guard_term = match_case
                .guard
                .as_ref()
                .filter(|p| *p != &Par::default())
                .map(|_| sorted_guard.term.clone());

            ScoredTerm {
                term: MatchCase {
                    pattern: Some(sorted_pattern.term),
                    source: Some(sorted_body.term),
                    free_count: match_case.free_count,
                    guard: guard_term,
                },
                score: Tree::Node(vec![
                    sorted_pattern.score,
                    sorted_body.score,
                    free_count_score,
                    sorted_guard.score,
                ]),
            }
        }

        let sorted_value = ParSortMatcher::sort_match(
            m.target
                .as_ref()
                .expect("target field on Match was None, should be Some"),
        );
        let scored_cases: Vec<ScoredTerm<MatchCase>> =
            m.cases.iter().map(|c| sort_case(c)).collect();
        let connective_used_score = if m.connective_used { 1 } else { 0 };

        ScoredTerm {
            term: Match {
                target: Some(sorted_value.term),
                cases: scored_cases.clone().into_iter().map(|c| c.term).collect(),
                locally_free: m.locally_free.clone(),
                connective_used: m.connective_used,
            },
            score: Tree::<ScoreAtom>::create_node_from_i32(
                Score::MATCH,
                vec![sorted_value.score]
                    .into_iter()
                    .chain(scored_cases.into_iter().map(|c| c.score))
                    .into_iter()
                    .chain(vec![Tree::<ScoreAtom>::create_leaf_from_i64(
                        connective_used_score,
                    )])
                    .collect(),
            ),
        }
    }
}
