// See models/src/main/scala/coop/rchain/models/rholang/sorter/ParSortMatcher.scala
//
// Leg-2 Stage C-2: the recursion moved to `sort_drive` (an explicit heap
// worklist) and the per-arm assembly to `sort_combine` (single-sourced with the
// recursive oracle). This file keeps what a caller sees.

// Changed by the merge of feature/cost-accounted-rho (DR-131): only the removed
// metered sorter used the names that this edit takes out of these imports.
// use super::send_sort_matcher::SendSortMatcher;
use super::sort_combine::split_scored_terms as split_scored_terms_impl;
use super::sort_drive::sort_par;
use super::sortable::Sortable;
// Changed by the merge of feature/cost-accounted-rho (DR-131): only the removed
// metered sorter used the names that this edit takes out of these imports.
// use crate::rhoapi::{
//     Bundle, Connective, CostSignedTerm, CostStack, Expr, GUnforgeable, If, Match, New, Par,
//     Receive, Send,
// };
use crate::rhoapi::Par;
// use crate::rust::rholang::sorter::bundle_sort_matcher::BundleSortMatcher;
// use crate::rust::rholang::sorter::connective_sort_matcher::ConnectiveSortMatcher;
// use crate::rust::rholang::sorter::expr_sort_matcher::ExprSortMatcher;
// use crate::rust::rholang::sorter::if_sort_matcher::IfSortMatcher;
// use crate::rust::rholang::sorter::match_sort_matcher::MatchSortMatcher;
// use crate::rust::rholang::sorter::new_sort_matcher::NewSortMatcher;
// use crate::rust::rholang::sorter::receive_sort_matcher::ReceiveSortMatcher;
// use crate::rust::rholang::sorter::score_tree::{EmittedBytes, Score, ScoreAtom, ScoredTerm, Tree};
use crate::rust::rholang::sorter::score_tree::{ScoreAtom, ScoredTerm, Tree};
// use crate::rust::rholang::sorter::unforgeable_sort_matcher::UnforgeableSortMatcher;

pub struct ParSortMatcher;

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
