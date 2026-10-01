// See models/src/main/scala/coop/rchain/models/rholang/sorter/SendSortMatcher.scala

use shared::rust::clone_backing::BackingError;

use super::metered::SorterMeter;
use super::par_sort_matcher::ParSortMatcher;
use super::score_tree::{Score, ScoreAtom, ScoredTerm, Tree};
use super::sortable::Sortable;
use crate::rhoapi::{Par, Send};

pub struct SendSortMatcher;

impl SendSortMatcher {
    pub fn sort_match_metered(
        send: &Send,
        meter: &SorterMeter<'_>,
    ) -> Result<ScoredTerm<Send>, BackingError> {
        let channel = ParSortMatcher::sort_match_metered(
            send.chan.as_ref().ok_or(BackingError::Rejected)?,
            meter,
        )?;
        let mut scored_data = meter.vec(send.data.len())?;
        for par in &send.data {
            scored_data.push(ParSortMatcher::sort_match_metered(par, meter)?);
        }
        let mut data = meter.vec(scored_data.len())?;
        let score_len = scored_data
            .len()
            .checked_add(3)
            .ok_or(BackingError::Overflow)?;
        let mut scores = meter.vec(score_len)?;
        scores.push(Tree::<ScoreAtom>::create_leaf_from_i64(
            send.persistent as i64,
        ));
        scores.push(channel.score);
        for scored in scored_data {
            data.push(scored.term);
            scores.push(scored.score);
        }
        scores.push(Tree::<ScoreAtom>::create_leaf_from_i64(
            send.connective_used as i64,
        ));
        Ok(ScoredTerm {
            term: Send {
                chan: Some(channel.term),
                data,
                persistent: send.persistent,
                locally_free: meter.clone(&send.locally_free)?,
                connective_used: send.connective_used,
            },
            score: Tree::<ScoreAtom>::create_node_from_i32_metered(Score::SEND, scores, meter)?,
        })
    }
}

impl Sortable<Send> for SendSortMatcher {
    fn sort_match(s: &Send) -> ScoredTerm<Send> {
        let sorted_chan = ParSortMatcher::sort_match(
            s.chan
                .as_ref()
                .expect("channel field on Send was None, should be Some"),
        );

        let sorted_data: Vec<ScoredTerm<Par>> = s
            .data
            .iter()
            .map(|p| ParSortMatcher::sort_match(p))
            .collect();

        let sorted_send = Send {
            chan: Some(sorted_chan.term),
            data: sorted_data.clone().into_iter().map(|p| p.term).collect(),
            persistent: s.persistent,
            locally_free: s.locally_free.clone(),
            connective_used: s.connective_used,
        };

        let persistent_score: i64 = if s.persistent { 1 } else { 0 };
        let connective_used_score: i64 = if s.connective_used { 1 } else { 0 };
        let send_score = Tree::<ScoreAtom>::create_node_from_i32(
            Score::SEND,
            vec![
                Tree::<ScoreAtom>::create_leaf_from_i64(persistent_score),
                sorted_chan.score,
            ]
            .into_iter()
            .chain(sorted_data.into_iter().map(|p| p.score))
            .chain(vec![Tree::<ScoreAtom>::create_leaf_from_i64(
                connective_used_score,
            )])
            .collect(),
        );

        ScoredTerm {
            term: sorted_send,
            score: send_score,
        }
    }
}
