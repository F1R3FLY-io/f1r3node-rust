// See models/src/main/scala/coop/rchain/models/rholang/sorter/ParSortMatcher.scala
//
// Leg-2 Stage C-2: the recursion moved to `sort_drive` (an explicit heap
// worklist) and the per-arm assembly to `sort_combine` (single-sourced with the
// recursive oracle). This file keeps what a caller sees.

use shared::rust::clone_backing::BackingError;

use super::metered::SorterMeter;
use super::send_sort_matcher::SendSortMatcher;
use super::sort_combine::split_scored_terms as split_scored_terms_impl;
use super::sort_drive::sort_par;
use super::sortable::Sortable;
use crate::rhoapi::{
    Bundle, Connective, CostSignedTerm, CostStack, Expr, GUnforgeable, If, Match, New, Par,
    Receive, Send,
};
use crate::rust::rholang::sorter::bundle_sort_matcher::BundleSortMatcher;
use crate::rust::rholang::sorter::connective_sort_matcher::ConnectiveSortMatcher;
use crate::rust::rholang::sorter::cost_accounting_sorter::{
    sort_signed_term_metered, sort_stack_metered,
};
use crate::rust::rholang::sorter::expr_sort_matcher::ExprSortMatcher;
use crate::rust::rholang::sorter::if_sort_matcher::IfSortMatcher;
use crate::rust::rholang::sorter::match_sort_matcher::MatchSortMatcher;
use crate::rust::rholang::sorter::new_sort_matcher::NewSortMatcher;
use crate::rust::rholang::sorter::receive_sort_matcher::ReceiveSortMatcher;
use crate::rust::rholang::sorter::score_tree::{EmittedBytes, Score, ScoreAtom, ScoredTerm, Tree};
use crate::rust::rholang::sorter::unforgeable_sort_matcher::UnforgeableSortMatcher;

pub struct ParSortMatcher;

impl ParSortMatcher {
    pub fn sort_match_metered(
        par: &Par,
        meter: &SorterMeter<'_>,
    ) -> Result<ScoredTerm<Par>, BackingError> {
        let _depth = meter.enter()?;
        let (sends, send_scores) =
            sort_group(&par.sends, meter, SendSortMatcher::sort_match_metered)?;
        let (receives, receive_scores) =
            sort_group(&par.receives, meter, ReceiveSortMatcher::sort_match_metered)?;
        let (exprs, expr_scores) =
            sort_group(&par.exprs, meter, ExprSortMatcher::sort_match_metered)?;
        let (news, new_scores) = sort_group(&par.news, meter, NewSortMatcher::sort_match_metered)?;
        let (matches, match_scores) =
            sort_group(&par.matches, meter, MatchSortMatcher::sort_match_metered)?;
        let (bundles, bundle_scores) =
            sort_group(&par.bundles, meter, BundleSortMatcher::sort_match_metered)?;
        let (connectives, connective_scores) = sort_group(
            &par.connectives,
            meter,
            ConnectiveSortMatcher::sort_match_metered,
        )?;
        let (unforgeables, unforgeable_scores) = sort_group(
            &par.unforgeables,
            meter,
            UnforgeableSortMatcher::sort_match_metered,
        )?;
        let (conditionals, conditional_scores) =
            sort_group(&par.conditionals, meter, IfSortMatcher::sort_match_metered)?;
        let (cost_signed_terms, cost_signed_scores) =
            sort_group(&par.cost_signed_terms, meter, sort_signed_term_metered)?;
        let (cost_stacks, cost_stack_scores) =
            sort_group(&par.cost_stacks, meter, sort_stack_metered)?;

        let score_count = [
            send_scores.len(),
            receive_scores.len(),
            expr_scores.len(),
            new_scores.len(),
            match_scores.len(),
            bundle_scores.len(),
            connective_scores.len(),
            unforgeable_scores.len(),
            conditional_scores.len(),
            cost_signed_scores.len(),
            cost_stack_scores.len(),
        ]
        .into_iter()
        .try_fold(1usize, |sum, size| sum.checked_add(size))
        .and_then(|sum| sum.checked_add(1))
        .ok_or(BackingError::Overflow)?;
        let mut scores = meter.vec(score_count)?;
        scores.push(Tree::<ScoreAtom>::create_leaf_from_i64(i64::from(
            Score::PAR,
        )));
        scores.extend(send_scores);
        scores.extend(receive_scores);
        scores.extend(expr_scores);
        scores.extend(new_scores);
        scores.extend(match_scores);
        scores.extend(bundle_scores);
        scores.extend(connective_scores);
        scores.extend(unforgeable_scores);
        scores.extend(conditional_scores);
        scores.extend(cost_signed_scores);
        scores.extend(cost_stack_scores);
        scores.push(Tree::<ScoreAtom>::create_leaf_from_i64(
            par.connective_used as i64,
        ));
        Ok(ScoredTerm {
            term: Par {
                sends,
                receives,
                news,
                exprs,
                matches,
                unforgeables,
                bundles,
                connectives,
                conditionals,
                locally_free: meter.clone(&par.locally_free)?,
                connective_used: par.connective_used,
                cost_signed_terms,
                cost_stacks,
            },
            score: Tree::Node(scores),
        })
    }
}

fn sort_group<T, F>(
    values: &[T],
    meter: &SorterMeter<'_>,
    mut sort: F,
) -> Result<(Vec<T>, Vec<Tree<ScoreAtom>>), BackingError>
where
    T: EmittedBytes,
    F: FnMut(&T, &SorterMeter<'_>) -> Result<ScoredTerm<T>, BackingError>,
{
    let mut scored = meter.vec(values.len())?;
    for value in values {
        scored.push(sort(value, meter)?);
    }
    ScoredTerm::sort_vec_metered(&mut scored, meter)?;
    let mut terms = meter.vec(scored.len())?;
    let mut scores = meter.vec(scored.len())?;
    for item in scored {
        terms.push(item.term);
        scores.push(item.score);
    }
    Ok((terms, scores))
}

impl Sortable<Par> for ParSortMatcher {
    /// Sort a `Par` into canonical form.
    ///
    /// ★ `cost_accounting/sig.rs` signs `sort_match(&par).term.encode_to_vec()`,
    /// so this function's output IS the consensus-visible canonical form. See
    /// `sort_drive`'s module documentation for the conversion's neutrality
    /// argument and the three checks behind it.
    fn sort_match(par: &Par) -> ScoredTerm<Par> { sort_par(par) }
}

/// Retained at its original path because callers outside the sorter use it.
/// The implementation lives in [`super::sort_combine`], which both the driver
/// and its recursive oracle share.
pub fn split_scored_terms<T>(scored_terms: Vec<ScoredTerm<T>>) -> (Vec<T>, Vec<Tree<ScoreAtom>>) {
    split_scored_terms_impl(scored_terms)
}
