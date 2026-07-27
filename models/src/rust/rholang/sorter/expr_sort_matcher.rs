// See models/src/main/scala/coop/rchain/models/rholang/sorter/ExprSortMatcher.scala
//
// Leg-2 Stage C-2: the 36-arm `ExprInstance` table moved to `sort_combine`
// (shared with the recursive oracle) and the recursion to `sort_drive` (an
// explicit heap worklist). Three arms — `ESetBody`, `EMapBody`,
// `EPathmapBody` — remain self-contained and run their own bounded drives;
// `sort_combine`'s module documentation says why, and the residual that leaves
// is measured by `stack_depth_gate.rs`'s `sort_nested_set` subject.

use super::score_tree::ScoredTerm;
use super::sort_drive::sort_expr;
use super::sortable::Sortable;
use crate::rhoapi::Expr;

pub struct ExprSortMatcher;

impl Sortable<Expr> for ExprSortMatcher {
    fn sort_match(e: &Expr) -> ScoredTerm<Expr> {
        sort_expr(e)
    }
}
