// See models/src/main/scala/coop/rchain/models/rholang/sorter/UnforgeableSortMatcher.scala
//
// A `GUnforgeable` carries bytes, never a `Par`, so it is a LEAF and needs no
// driver. The implementation lives in `sort_combine`.

use super::score_tree::ScoredTerm;
use super::sort_combine::sort_unforgeable;
use super::sortable::Sortable;
use crate::rhoapi::GUnforgeable;

pub struct UnforgeableSortMatcher;

impl Sortable<GUnforgeable> for UnforgeableSortMatcher {
    fn sort_match(unf: &GUnforgeable) -> ScoredTerm<GUnforgeable> { sort_unforgeable(unf) }
}
