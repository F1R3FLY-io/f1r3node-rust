use super::score_tree::ScoredTerm;
use super::sort_drive::sort_expr;
use super::sortable::Sortable;
use crate::rhoapi::Expr;

pub struct ExprSortMatcher;

impl Sortable<Expr> for ExprSortMatcher {
    fn sort_match(e: &Expr) -> ScoredTerm<Expr> { sort_expr(e) }
}
