// See models/src/main/scala/coop/rchain/models/rholang/sorter/VarSortMatcher.scala
//
// A `Var` has no sub-`Par`, so it is a LEAF for every traversal in this family
// and needs no driver. The implementation lives in `sort_combine` so that the
// driver, the recursive oracle and this entry point cannot disagree.

// Changed by the merge of feature/cost-accounted-rho (DR-131): only the removed
// metered sorter used the names that this edit takes out of these imports.
// use super::score_tree::{Score, ScoreAtom, ScoredTerm, Tree};
use super::score_tree::ScoredTerm;
use super::sort_combine::sort_var;
use super::sortable::Sortable;
// Changed by the merge of feature/cost-accounted-rho (DR-131): only the removed
// metered sorter used the names that this edit takes out of these imports.
// use crate::rhoapi::var::VarInstance;
use crate::rhoapi::Var;

pub struct VarSortMatcher;

impl Sortable<Var> for VarSortMatcher {
    fn sort_match(v: &Var) -> ScoredTerm<Var> { sort_var(v) }
}
