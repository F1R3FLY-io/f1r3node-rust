// See models/src/main/scala/coop/rchain/models/rholang/sorter/ParSortMatcher.scala
//
// Leg-2 Stage C-2: the recursion moved to `sort_drive` (an explicit heap
// worklist) and the per-arm assembly to `sort_combine` (single-sourced with the
// recursive oracle). This file keeps what a caller sees.

use super::score_tree::{ScoreAtom, ScoredTerm, Tree};
use super::sort_combine::split_scored_terms as split_scored_terms_impl;
use super::sort_drive::sort_par;
use super::sortable::Sortable;
use crate::rhoapi::Par;

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
