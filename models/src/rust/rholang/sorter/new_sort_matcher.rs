// See models/src/main/scala/coop/rchain/models/rholang/sorter/NewSortMatcher.scala

use shared::rust::clone_backing::BackingError;

use super::metered::SorterMeter;
use super::par_sort_matcher::ParSortMatcher;
use super::score_tree::{ScoreAtom, ScoredTerm, Tree};
use super::sortable::Sortable;
use crate::rhoapi::{New, Par};
use crate::rust::rholang::sorter::score_tree::Score;

pub struct NewSortMatcher;

impl NewSortMatcher {
    pub fn sort_match_metered(
        value: &New,
        meter: &SorterMeter<'_>,
    ) -> Result<ScoredTerm<New>, BackingError> {
        let _depth = meter.enter()?;
        let sorted_body = ParSortMatcher::sort_match_metered(
            value.p.as_ref().ok_or(BackingError::Rejected)?,
            meter,
        )?;
        let mut uris = meter.vec(value.uri.len())?;
        for uri in &value.uri {
            uris.push(ScoredTerm {
                term: meter.clone(uri)?,
                score: Tree::<ScoreAtom>::create_leaf_from_string(meter.clone(uri)?),
            });
        }
        ScoredTerm::sort_vec_metered(&mut uris, meter)?;
        let mut sorted_uri = meter.vec(uris.len())?;
        let mut uri_scores = meter.vec(uris.len().max(1))?;
        for uri in uris {
            sorted_uri.push(uri.term);
            uri_scores.push(uri.score);
        }
        if uri_scores.is_empty() {
            uri_scores.push(Tree::<ScoreAtom>::create_leaf_from_i64(i64::from(
                Score::ABSENT,
            )));
        }

        let injections_list = meter.clone(&value.injections)?;
        let mut injection_scores = meter.vec(injections_list.len().max(1))?;
        for (key, par) in &injections_list {
            let scored = ParSortMatcher::sort_match_metered(par, meter)?;
            let mut children = meter.vec(2)?;
            children.push(Tree::<ScoreAtom>::create_leaf_from_string(
                meter.clone(key)?,
            ));
            children.push(scored.score);
            injection_scores.push(Tree::Node(children));
        }
        if injection_scores.is_empty() {
            injection_scores.push(Tree::<ScoreAtom>::create_leaf_from_i64(i64::from(
                Score::ABSENT,
            )));
        }
        let score_count = uri_scores
            .len()
            .checked_add(injection_scores.len())
            .and_then(|count| count.checked_add(3))
            .ok_or(BackingError::Overflow)?;
        let mut scores = meter.vec(score_count)?;
        scores.push(Tree::<ScoreAtom>::create_leaf_from_i64(i64::from(
            Score::NEW,
        )));
        scores.push(Tree::<ScoreAtom>::create_leaf_from_i64(i64::from(
            value.bind_count,
        )));
        scores.extend(uri_scores);
        scores.extend(injection_scores);
        scores.push(sorted_body.score);
        Ok(ScoredTerm {
            term: New {
                bind_count: value.bind_count,
                p: Some(sorted_body.term),
                uri: sorted_uri,
                injections: meter.clone(&value.injections)?,
                locally_free: meter.clone(&value.locally_free)?,
            },
            score: Tree::Node(scores),
        })
    }
}

impl Sortable<New> for NewSortMatcher {
    fn sort_match(n: &New) -> ScoredTerm<New> {
        let sorted_par = ParSortMatcher::sort_match(
            n.p.as_ref()
                .expect("p field on New was None, should be Some"),
        );

        let mut sorted_uri = n.uri.clone();
        sorted_uri.sort();

        let uri_score = if !sorted_uri.is_empty() {
            sorted_uri
                .clone()
                .into_iter()
                .map(|s| Tree::<ScoreAtom>::create_leaf_from_string(s))
                .collect()
        } else {
            vec![Tree::<ScoreAtom>::create_leaf_from_i64(
                Score::ABSENT as i64,
            )]
        };

        let injections_list: Vec<(String, Par)> = n.injections.clone().into_iter().collect();
        let injections_score = if !injections_list.is_empty() {
            injections_list
                .iter()
                .map(|(k, v)| {
                    let scored_term = ParSortMatcher::sort_match(v);
                    Tree::<ScoreAtom>::create_node_from_string(k.clone(), vec![scored_term.score])
                })
                .collect()
        } else {
            vec![Tree::<ScoreAtom>::create_leaf_from_i64(
                Score::ABSENT as i64,
            )]
        };

        ScoredTerm {
            term: New {
                bind_count: n.bind_count,
                p: Some(sorted_par.term),
                uri: sorted_uri,
                injections: n.injections.clone(),
                locally_free: n.locally_free.clone(),
            },
            score: Tree::Node(
                std::iter::once(Tree::<ScoreAtom>::create_leaf_from_i64(Score::NEW as i64))
                    .chain(std::iter::once(Tree::<ScoreAtom>::create_leaf_from_i64(
                        n.bind_count as i64,
                    )))
                    .chain(uri_score.into_iter())
                    .chain(injections_score.into_iter())
                    .chain(std::iter::once(sorted_par.score))
                    .collect(),
            ),
        }
    }
}
