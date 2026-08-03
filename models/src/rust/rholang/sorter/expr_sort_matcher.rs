// See models/src/main/scala/coop/rchain/models/rholang/sorter/ExprSortMatcher.scala
//
// Leg-2 Stage C-2: the 36-arm `ExprInstance` table moved to `sort_combine`
// (shared with the recursive oracle) and the recursion to `sort_drive` (an
// explicit heap worklist). `ESetBody`, `EMapBody`, and both homogeneous
// `EPathMap` modes expose their members to that same machine; their combines
// consume already-scored children without recursive sorter re-entry.

use super::score_tree::ScoredTerm;
use super::sort_drive::sort_expr;
use super::sortable::Sortable;
use crate::rhoapi::Expr;

pub struct ExprSortMatcher;

impl Sortable<Expr> for ExprSortMatcher {
    fn sort_match(e: &Expr) -> ScoredTerm<Expr> { sort_expr(e) }
}
