// See models/src/main/scala/coop/rchain/models/rholang/sorter/VarSortMatcher.scala
//
// A `Var` has no sub-`Par`, so it is a LEAF for every traversal in this family
// and needs no driver. The implementation lives in `sort_combine` so that the
// driver, the recursive oracle and this entry point cannot disagree.

use super::score_tree::ScoredTerm;
use super::sort_combine::sort_var;
use super::sortable::Sortable;
use crate::rhoapi::Var;

pub struct VarSortMatcher;

impl Sortable<Var> for VarSortMatcher {
    fn sort_match(v: &Var) -> ScoredTerm<Var> { sort_var(v) }
}
