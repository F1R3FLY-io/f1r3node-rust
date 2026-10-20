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
// Changed by the merge of feature/cost-accounted-rho (DR-131): only the removed
// metered sorter used the names that this edit takes out of these imports.
// use crate::rhoapi::expr::ExprInstance;
// use crate::rhoapi::{
//     EAnd, EDiv, EEq, EGt, EGte, EList, ELt, ELte, EMatches, EMinus, EMinusMinus, EMod, EMult, ENeg,
//     ENeq, ENot, EOr, EPathMap, EPercentPercent, EPlus, EPlusPlus, EVar, EZipper, Expr, Par, Var,
// };
use crate::rhoapi::Expr;
// use crate::rust::canonical_path::decode_trie_path;
// use crate::rust::epathmap_trie_codec::EPathMapMode;
// use crate::rust::par_map::ParMap;
// use crate::rust::par_map_type_mapper::ParMapTypeMapper;
// use crate::rust::par_set::ParSet;
// use crate::rust::par_set_type_mapper::ParSetTypeMapper;
// use crate::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
// use crate::rust::rholang::sorter::score_tree::{Score, ScoreAtom, Tree};
// use crate::rust::rholang::sorter::var_sort_matcher::VarSortMatcher;
// use crate::rust::sorted_par_hash_set::SortedParHashSet;

pub struct ExprSortMatcher;

impl Sortable<Expr> for ExprSortMatcher {
    fn sort_match(e: &Expr) -> ScoredTerm<Expr> { sort_expr(e) }
}
