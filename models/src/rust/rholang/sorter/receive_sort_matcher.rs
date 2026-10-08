// See models/src/main/scala/coop/rchain/models/rholang/sorter/ReceiveSortMatcher.scala

use shared::rust::clone_backing::BackingError;

use super::cost_accounting_sorter::{sort_signature, sort_signature_metered};
use super::metered::SorterMeter;
use super::par_sort_matcher::ParSortMatcher;
use super::score_tree::{Score, ScoreAtom, ScoredTerm, Tree};
use super::sortable::Sortable;
use super::var_sort_matcher::VarSortMatcher;
use crate::rhoapi::{Par, Receive, ReceiveBind};

pub struct ReceiveSortMatcher;

impl ReceiveSortMatcher {
    pub fn sort_bind_metered(
        bind: &ReceiveBind,
        meter: &SorterMeter<'_>,
    ) -> Result<ScoredTerm<ReceiveBind>, BackingError> {
        let _depth = meter.enter()?;
        let mut patterns = meter.vec(bind.patterns.len())?;
        // Changed by D-E4 (DR-111): Rule S, one read of each slot.
        // let mut pattern_scores = meter.vec(bind.patterns.len())?;
        let mut pattern_scores = meter.score_vec(bind.patterns.len())?;
        for pattern in &bind.patterns {
            let scored = ParSortMatcher::sort_match_metered(pattern, meter)?;
            patterns.push(scored.term);
            pattern_scores.push(scored.score);
        }
        let channel = ParSortMatcher::sort_match_metered(
            bind.source.as_ref().ok_or(BackingError::Rejected)?,
            meter,
        )?;
        let (remainder, remainder_score) = match &bind.remainder {
            Some(value) => {
                let scored = VarSortMatcher::sort_match_metered(value, meter)?;
                (Some(scored.term), scored.score)
            }
            None => (
                None,
                Tree::<ScoreAtom>::create_leaf_from_i64(i64::from(Score::ABSENT)),
            ),
        };
        let signature = bind
            .cost_signature
            .as_ref()
            .map(|value| sort_signature_metered(value, meter))
            .transpose()?;
        // Changed by D-E4 (DR-111): Rule S, one read of each slot.
        // let mut scores = meter.vec(
        let mut scores = meter.score_vec(
            pattern_scores
                .len()
                .checked_add(2)
                .and_then(|len| len.checked_add(usize::from(signature.is_some())))
                .ok_or(BackingError::Overflow)?,
        )?;
        scores.push(channel.score);
        scores.extend(pattern_scores);
        scores.push(remainder_score);
        let cost_signature = signature.map(|scored| {
            scores.push(scored.score);
            scored.term
        });
        Ok(ScoredTerm {
            term: ReceiveBind {
                patterns,
                source: Some(channel.term),
                remainder,
                free_count: bind.free_count,
                cost_signature,
            },
            score: Tree::Node(scores),
        })
    }

    pub fn sort_bind(bind: ReceiveBind) -> ScoredTerm<ReceiveBind> {
        let patterns = bind.patterns;
        let source = bind
            .source
            .expect("source field on Bind was None, should be Some");

        let sorted_patterns: Vec<ScoredTerm<Par>> = patterns
            .into_iter()
            .map(|p| ParSortMatcher::sort_match(&p))
            .collect();
        let sorted_channel = ParSortMatcher::sort_match(&source);
        let sorted_remainder = match &bind.remainder {
            Some(bind_remainder) => {
                let scored_var = VarSortMatcher::sort_match(bind_remainder);
                ScoredTerm {
                    term: Some(scored_var.term),
                    score: scored_var.score,
                }
            }
            None => ScoredTerm {
                term: None,
                score: Tree::<ScoreAtom>::create_leaf_from_i64(Score::ABSENT as i64),
            },
        };
        let sorted_cost_signature = bind.cost_signature.as_ref().map(sort_signature);

        ScoredTerm {
            term: ReceiveBind {
                patterns: sorted_patterns
                    .clone()
                    .into_iter()
                    .map(|p| p.term)
                    .collect(),
                source: Some(sorted_channel.term),
                remainder: bind.remainder,
                free_count: bind.free_count,
                cost_signature: sorted_cost_signature
                    .as_ref()
                    .map(|signature| signature.term.clone()),
            },
            score: Tree::Node(
                vec![sorted_channel.score]
                    .into_iter()
                    .chain(sorted_patterns.into_iter().map(|p| p.score))
                    .chain(vec![sorted_remainder.score].into_iter())
                    .chain(
                        sorted_cost_signature
                            .into_iter()
                            .map(|signature| signature.score),
                    )
                    .collect(),
            ),
        }
    }
}

impl ReceiveSortMatcher {
    pub fn sort_match_metered(
        value: &Receive,
        meter: &SorterMeter<'_>,
    ) -> Result<ScoredTerm<Receive>, BackingError> {
        let _depth = meter.enter()?;
        let mut binds = meter.vec(value.binds.len())?;
        // Changed by D-E4 (DR-111): Rule S, one read of each slot.
        // let mut bind_scores = meter.vec(value.binds.len())?;
        let mut bind_scores = meter.score_vec(value.binds.len())?;
        for bind in &value.binds {
            let scored = Self::sort_bind_metered(bind, meter)?;
            binds.push(scored.term);
            bind_scores.push(scored.score);
        }
        let body = ParSortMatcher::sort_match_metered(
            value.body.as_ref().ok_or(BackingError::Rejected)?,
            meter,
        )?;
        let empty = Par::default();
        let condition =
            ParSortMatcher::sort_match_metered(value.condition.as_ref().unwrap_or(&empty), meter)?;
        let condition_term = value
            .condition
            .as_ref()
            .filter(|par| *par != &empty)
            .map(|_| condition.term);
        // Changed by D-E4 (DR-111): Rule S, one read of each slot.
        // let mut scores = meter.vec(
        let mut scores = meter.score_vec(
            bind_scores
                .len()
                .checked_add(6)
                .ok_or(BackingError::Overflow)?,
        )?;
        scores.push(Tree::<ScoreAtom>::create_leaf_from_i64(
            value.persistent as i64,
        ));
        scores.push(Tree::<ScoreAtom>::create_leaf_from_i64(value.peek as i64));
        scores.extend(bind_scores);
        scores.push(body.score);
        scores.push(Tree::<ScoreAtom>::create_leaf_from_i64(i64::from(
            value.bind_count,
        )));
        scores.push(Tree::<ScoreAtom>::create_leaf_from_i64(
            value.connective_used as i64,
        ));
        scores.push(condition.score);
        Ok(ScoredTerm {
            term: Receive {
                binds,
                body: Some(body.term),
                persistent: value.persistent,
                peek: value.peek,
                bind_count: value.bind_count,
                // Changed by D-O1 (DR-111): block accounting.
                // locally_free: meter.clone(&value.locally_free)?,
                locally_free: meter.clone_blocks(&value.locally_free)?,
                connective_used: value.connective_used,
                condition: condition_term,
            },
            score: Tree::<ScoreAtom>::create_node_from_i32_metered(Score::RECEIVE, scores, meter)?,
        })
    }
}

impl Sortable<Receive> for ReceiveSortMatcher {
    // The order of the binds must already be presorted by the time this is called.
    // This function will then sort the insides of the preordered binds.
    fn sort_match(r: &Receive) -> ScoredTerm<Receive> {
        let sorted_binds: Vec<ScoredTerm<ReceiveBind>> = r
            .binds
            .clone()
            .into_iter()
            .map(|rb| ReceiveSortMatcher::sort_bind(rb))
            .collect();

        let persistent_score: i64 = if r.persistent { 1 } else { 0 };
        let peek_score: i64 = if r.peek { 1 } else { 0 };
        let connective_used_score: i64 = if r.connective_used { 1 } else { 0 };
        let sorted_body = ParSortMatcher::sort_match(
            r.body
                .as_ref()
                .expect("body field on Receive was None, should be Some"),
        );

        // Optional `where`-clause condition. Empty Par when absent so the
        // score is stable. Collapse `Some(empty Par)` to `None` on the
        // output term so the wire format doesn't preserve a distinction
        // the runtime treats as identical (eval_receive ignores empty).
        let condition_par = r.condition.clone().unwrap_or_default();
        let sorted_condition = ParSortMatcher::sort_match(&condition_par);
        let condition_term = r
            .condition
            .as_ref()
            .filter(|p| *p != &Par::default())
            .map(|_| sorted_condition.term.clone());

        ScoredTerm {
            term: Receive {
                binds: sorted_binds.clone().into_iter().map(|rb| rb.term).collect(),
                body: Some(sorted_body.term),
                persistent: r.persistent,
                peek: r.peek,
                bind_count: r.bind_count,
                locally_free: r.locally_free.clone(),
                connective_used: r.connective_used,
                condition: condition_term,
            },
            score: Tree::<ScoreAtom>::create_node_from_i32(
                Score::RECEIVE,
                vec![
                    Tree::<ScoreAtom>::create_leaf_from_i64(persistent_score),
                    Tree::<ScoreAtom>::create_leaf_from_i64(peek_score),
                ]
                .into_iter()
                .chain(sorted_binds.into_iter().map(|rb| rb.score))
                .chain(vec![sorted_body.score])
                .chain(vec![
                    Tree::<ScoreAtom>::create_leaf_from_i64(r.bind_count as i64),
                    Tree::<ScoreAtom>::create_leaf_from_i64(connective_used_score),
                    sorted_condition.score,
                ])
                .collect(),
            ),
        }
    }
}
