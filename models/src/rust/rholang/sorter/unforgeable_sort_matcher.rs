// See models/src/main/scala/coop/rchain/models/rholang/sorter/UnforgeableSortMatcher.scala
//
// A `GUnforgeable` carries bytes, never a `Par`, so it is a LEAF and needs no
// driver. The implementation lives in `sort_combine`.

// Changed by the merge of feature/cost-accounted-rho (DR-131): only the removed
// metered sorter used the names that this edit takes out of these imports.
// use super::score_tree::{Score, ScoreAtom, ScoredTerm, Tree};
use super::score_tree::ScoredTerm;
use super::sort_combine::sort_unforgeable;
use super::sortable::Sortable;
// Changed by the merge of feature/cost-accounted-rho (DR-131): only the removed
// metered sorter used the names that this edit takes out of these imports.
// use crate::rhoapi::g_unforgeable::UnfInstance;
use crate::rhoapi::GUnforgeable;

pub struct UnforgeableSortMatcher;

impl Sortable<GUnforgeable> for UnforgeableSortMatcher {
    fn sort_match(unf: &GUnforgeable) -> ScoredTerm<GUnforgeable> { sort_unforgeable(unf) }
}
