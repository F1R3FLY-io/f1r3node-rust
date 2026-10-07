// See models/src/main/scala/coop/rchain/models/rholang/sorter/ReceiveSortMatcher.scala
//
// Leg-2 Stage C-2: see `sort_drive` / `sort_combine`.

use shared::rust::clone_backing::BackingError;

use super::cost_accounting_sorter::sort_signature_metered;
use super::metered::SorterMeter;
use super::par_sort_matcher::ParSortMatcher;
use super::score_tree::{Score, ScoreAtom, ScoredTerm, Tree};
use super::sort_drive::{sort_bind_ref, sort_receive};
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
        let mut pattern_scores = meter.vec(bind.patterns.len())?;
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
        let mut scores = meter.vec(
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

    /// Sort one bind.
    ///
    /// The by-value signature is retained because callers outside the sorter
    /// use it; the driver borrows, so the pre-conversion `r.binds.clone()` — a
    /// Θ(depth) `<Par as Clone>::clone` of every pattern and source, once per
    /// receive — is gone from the internal path.
    pub fn sort_bind(bind: ReceiveBind) -> ScoredTerm<ReceiveBind> { sort_bind_ref(&bind) }

    /// Sort one bind without taking ownership. This is what
    /// [`Sortable<Receive>::sort_match`] uses.
    pub fn sort_bind_by_ref(bind: &ReceiveBind) -> ScoredTerm<ReceiveBind> { sort_bind_ref(bind) }
}

impl ReceiveSortMatcher {
    pub fn sort_match_metered(
        value: &Receive,
        meter: &SorterMeter<'_>,
    ) -> Result<ScoredTerm<Receive>, BackingError> {
        let _depth = meter.enter()?;
        let mut binds = meter.vec(value.binds.len())?;
        let mut bind_scores = meter.vec(value.binds.len())?;
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
        let mut scores = meter.vec(
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
                locally_free: meter.clone(&value.locally_free)?,
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
    fn sort_match(r: &Receive) -> ScoredTerm<Receive> { sort_receive(r) }
}
