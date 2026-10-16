// See models/src/main/scala/coop/rchain/models/rholang/sorter/ExprSortMatcher.scala
//
// Leg-2 Stage C-2: the 36-arm `ExprInstance` table moved to `sort_combine`
// (shared with the recursive oracle) and the recursion to `sort_drive` (an
// explicit heap worklist). `ESetBody`, `EMapBody`, and both homogeneous
// `EPathMap` modes expose their members to that same machine; their combines
// consume already-scored children without recursive sorter re-entry.

use shared::rust::clone_backing::BackingError;

use super::metered::SorterMeter;
use super::score_tree::ScoredTerm;
use super::sort_combine::canonicalize_scored_pathmap;
use super::sort_drive::sort_expr;
use super::sortable::Sortable;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::{
    EAnd, EDiv, EEq, EGt, EGte, EList, ELt, ELte, EMatches, EMinus, EMinusMinus, EMod, EMult, ENeg,
    ENeq, ENot, EOr, EPathMap, EPercentPercent, EPlus, EPlusPlus, EVar, EZipper, Expr, Par, Var,
};
use crate::rust::canonical_path::decode_trie_path;
use crate::rust::epathmap_trie_codec::EPathMapMode;
use crate::rust::par_map::ParMap;
use crate::rust::par_map_type_mapper::ParMapTypeMapper;
use crate::rust::par_set::ParSet;
use crate::rust::par_set_type_mapper::ParSetTypeMapper;
use crate::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use crate::rust::rholang::sorter::score_tree::{Score, ScoreAtom, Tree};
use crate::rust::rholang::sorter::var_sort_matcher::VarSortMatcher;
use crate::rust::sorted_par_hash_set::SortedParHashSet;

pub struct ExprSortMatcher;

/// Target-only trie tariff. Source Vec<Par> charges are unchanged for every
/// other expression arm; a trie must pay for key decoding and canonical
/// re-filing before either operation allocates. The canonicalizer below owns
/// the actual set/map semantics, including deduplication and trie score order.
fn sort_pathmap_metered(
    pathmap: &EPathMap,
    meter: &SorterMeter<'_>,
    preserve_stable_storage: bool,
) -> Result<(EPathMap, Vec<Tree<ScoreAtom>>), BackingError> {
    let capacity = match pathmap.mode() {
        EPathMapMode::Map => pathmap.len().checked_mul(2),
        EPathMapMode::Empty | EPathMapMode::Set => Some(pathmap.len()),
    }
    .ok_or(BackingError::Overflow)?;
    let mut children = meter.vec(capacity)?;
    let mut key_bytes = 0usize;
    let mut failure = None;

    match pathmap.mode() {
        EPathMapMode::Empty => {}
        EPathMapMode::Set => {
            pathmap
                .entry_trie()
                .for_each_raw_set_entry(|key| {
                    if failure.is_some() {
                        return;
                    }
                    let result = (|| {
                        meter.reserve(1, key.len(), key.len())?;
                        key_bytes = key_bytes
                            .checked_add(key.len())
                            .ok_or(BackingError::Overflow)?;
                        let decoded = decode_trie_path(key).map_err(|_| BackingError::Rejected)?;
                        let sorted = ParSortMatcher::sort_match_metered(&decoded, meter);
                        crate::rust::rholang::par_children::dismantle(decoded);
                        meter.push(&mut children, sorted?)?;
                        Ok(())
                    })();
                    if let Err(error) = result {
                        failure = Some(error);
                    }
                })
                .map_err(|_| BackingError::Rejected)?;
        }
        EPathMapMode::Map => {
            pathmap
                .entry_trie()
                .for_each_raw_map_entry(|key, value| {
                    if failure.is_some() {
                        return;
                    }
                    let result = (|| {
                        meter.reserve(1, key.len(), key.len())?;
                        key_bytes = key_bytes
                            .checked_add(key.len())
                            .ok_or(BackingError::Overflow)?;
                        let decoded = decode_trie_path(key).map_err(|_| BackingError::Rejected)?;
                        let sorted_key = ParSortMatcher::sort_match_metered(&decoded, meter);
                        crate::rust::rholang::par_children::dismantle(decoded);
                        meter.push(&mut children, sorted_key?)?;
                        meter.push(
                            &mut children,
                            ParSortMatcher::sort_match_metered(value, meter)?,
                        )?;
                        Ok(())
                    })();
                    if let Err(error) = result {
                        failure = Some(error);
                    }
                })
                .map_err(|_| BackingError::Rejected)?;
        }
    }
    if let Some(error) = failure {
        return Err(error);
    }

    // Prepay the target-only canonical re-file: one input-root copy/cleanup,
    // one score slot per sorted child, and key-index work proportional to the
    // canonical input key bytes. No trie mutation occurs before this reserve.
    shared::rust::clone_backing::reserve_copy_and_cleanup(pathmap, meter)?;
    let score_bytes = children
        .len()
        .checked_mul(std::mem::size_of::<Tree<ScoreAtom>>())
        .ok_or(BackingError::Overflow)?;
    let trie_bytes = key_bytes
        .checked_mul(128)
        .and_then(|bytes| bytes.checked_add(pathmap.len().checked_mul(256)?))
        .ok_or(BackingError::Overflow)?;
    meter.reserve(
        children.len(),
        score_bytes
            .checked_add(trie_bytes)
            .ok_or(BackingError::Overflow)?,
        score_bytes
            .checked_add(trie_bytes)
            .ok_or(BackingError::Overflow)?,
    )?;
    Ok(canonicalize_scored_pathmap(
        pathmap,
        children,
        preserve_stable_storage,
    ))
}

impl Sortable<Expr> for ExprSortMatcher {
    fn sort_match(e: &Expr) -> ScoredTerm<Expr> { sort_expr(e) }
}

fn construct_metered(expr_instance: ExprInstance, score: Tree<ScoreAtom>) -> ScoredTerm<Expr> {
    ScoredTerm {
        term: Expr {
            expr_instance: Some(expr_instance),
        },
        score,
    }
}

fn score_children(
    first: Tree<ScoreAtom>,
    second: Tree<ScoreAtom>,
    meter: &SorterMeter<'_>,
) -> Result<Vec<Tree<ScoreAtom>>, BackingError> {
    let mut scores = meter.vec(2)?;
    scores.push(first);
    scores.push(second);
    Ok(scores)
}

fn score_node(
    kind: i32,
    score: Tree<ScoreAtom>,
    meter: &SorterMeter<'_>,
) -> Result<Tree<ScoreAtom>, BackingError> {
    let mut children = meter.vec(1)?;
    children.push(score);
    Tree::<ScoreAtom>::create_node_from_i32_metered(kind, children, meter)
}

fn sort_pars_metered(
    pars: &[Par],
    meter: &SorterMeter<'_>,
) -> Result<Vec<ScoredTerm<Par>>, BackingError> {
    let mut result = meter.vec(pars.len())?;
    for par in pars {
        result.push(ParSortMatcher::sort_match_metered(par, meter)?);
    }
    Ok(result)
}

fn split_pars_metered(
    pars: Vec<ScoredTerm<Par>>,
    meter: &SorterMeter<'_>,
) -> Result<(Vec<Par>, Vec<Tree<ScoreAtom>>), BackingError> {
    let mut terms = meter.vec(pars.len())?;
    let mut scores = meter.vec(pars.len())?;
    for par in pars {
        terms.push(par.term);
        scores.push(par.score);
    }
    Ok((terms, scores))
}

fn collection_score(
    kind: i32,
    remainder: &Option<Var>,
    elements: Vec<Tree<ScoreAtom>>,
    connective_used: bool,
    meter: &SorterMeter<'_>,
) -> Result<Tree<ScoreAtom>, BackingError> {
    let count = elements
        .len()
        .checked_add(3)
        .ok_or(BackingError::Overflow)?;
    let mut scores = meter.vec(count)?;
    scores.push(Tree::<ScoreAtom>::create_leaf_from_i64(kind as i64));
    scores.push(match remainder {
        Some(var) => VarSortMatcher::sort_match_metered(var, meter)?.score,
        None => Tree::<ScoreAtom>::create_leaf_from_i64(-1),
    });
    scores.extend(elements);
    scores.push(Tree::<ScoreAtom>::create_leaf_from_i64(i64::from(
        connective_used,
    )));
    Ok(Tree::Node(scores))
}

macro_rules! sort_binary_expr_metered {
    ($value:expr, $variant:ident, $ty:ident, $score:expr, $meter:expr) => {{
        let left = ParSortMatcher::sort_match_metered(
            $value.p1.as_ref().ok_or(BackingError::Rejected)?,
            $meter,
        )?;
        let right = ParSortMatcher::sort_match_metered(
            $value.p2.as_ref().ok_or(BackingError::Rejected)?,
            $meter,
        )?;
        Ok(construct_metered(
            ExprInstance::$variant($ty {
                p1: Some(left.term),
                p2: Some(right.term),
            }),
            Tree::<ScoreAtom>::create_node_from_i32_metered(
                $score,
                score_children(left.score, right.score, $meter)?,
                $meter,
            )?,
        ))
    }};
}

impl ExprSortMatcher {
    pub fn sort_match_metered(
        expr: &Expr,
        meter: &SorterMeter<'_>,
    ) -> Result<ScoredTerm<Expr>, BackingError> {
        let _depth = meter.enter()?;
        meter.reserve(1, std::mem::size_of::<Expr>(), 0)?;
        match &expr.expr_instance {
            Some(ExprInstance::ENegBody(value)) => {
                let sorted = ParSortMatcher::sort_match_metered(
                    value.p.as_ref().ok_or(BackingError::Rejected)?,
                    meter,
                )?;
                Ok(construct_metered(
                    ExprInstance::ENegBody(ENeg {
                        p: Some(sorted.term),
                    }),
                    score_node(Score::ENEG, sorted.score, meter)?,
                ))
            }
            Some(ExprInstance::ENotBody(value)) => {
                let sorted = ParSortMatcher::sort_match_metered(
                    value.p.as_ref().ok_or(BackingError::Rejected)?,
                    meter,
                )?;
                Ok(construct_metered(
                    ExprInstance::ENotBody(ENot {
                        p: Some(sorted.term),
                    }),
                    score_node(Score::ENOT, sorted.score, meter)?,
                ))
            }
            Some(ExprInstance::EVarBody(value)) => {
                let sorted = VarSortMatcher::sort_match_metered(
                    value.v.as_ref().ok_or(BackingError::Rejected)?,
                    meter,
                )?;
                Ok(construct_metered(
                    ExprInstance::EVarBody(EVar {
                        v: Some(sorted.term),
                    }),
                    score_node(Score::EVAR, sorted.score, meter)?,
                ))
            }
            Some(ExprInstance::EMultBody(value)) => {
                sort_binary_expr_metered!(value, EMultBody, EMult, Score::EMULT, meter)
            }
            Some(ExprInstance::EDivBody(value)) => {
                sort_binary_expr_metered!(value, EDivBody, EDiv, Score::EDIV, meter)
            }
            Some(ExprInstance::EModBody(value)) => {
                sort_binary_expr_metered!(value, EModBody, EMod, Score::EMOD, meter)
            }
            Some(ExprInstance::EPlusBody(value)) => {
                sort_binary_expr_metered!(value, EPlusBody, EPlus, Score::EPLUS, meter)
            }
            Some(ExprInstance::EMinusBody(value)) => {
                sort_binary_expr_metered!(value, EMinusBody, EMinus, Score::EMINUS, meter)
            }
            Some(ExprInstance::ELtBody(value)) => {
                sort_binary_expr_metered!(value, ELtBody, ELt, Score::ELT, meter)
            }
            Some(ExprInstance::ELteBody(value)) => {
                sort_binary_expr_metered!(value, ELteBody, ELte, Score::ELTE, meter)
            }
            Some(ExprInstance::EGtBody(value)) => {
                sort_binary_expr_metered!(value, EGtBody, EGt, Score::EGT, meter)
            }
            Some(ExprInstance::EGteBody(value)) => {
                sort_binary_expr_metered!(value, EGteBody, EGte, Score::EGTE, meter)
            }
            Some(ExprInstance::EEqBody(value)) => {
                sort_binary_expr_metered!(value, EEqBody, EEq, Score::EEQ, meter)
            }
            Some(ExprInstance::ENeqBody(value)) => {
                sort_binary_expr_metered!(value, ENeqBody, ENeq, Score::ENEQ, meter)
            }
            Some(ExprInstance::EAndBody(value)) => {
                sort_binary_expr_metered!(value, EAndBody, EAnd, Score::EAND, meter)
            }
            Some(ExprInstance::EOrBody(value)) => {
                sort_binary_expr_metered!(value, EOrBody, EOr, Score::EOR, meter)
            }
            Some(ExprInstance::EPercentPercentBody(value)) => {
                sort_binary_expr_metered!(
                    value,
                    EPercentPercentBody,
                    EPercentPercent,
                    Score::EPERCENT,
                    meter
                )
            }
            Some(ExprInstance::EPlusPlusBody(value)) => {
                sort_binary_expr_metered!(value, EPlusPlusBody, EPlusPlus, Score::EPLUSPLUS, meter)
            }
            Some(ExprInstance::EMinusMinusBody(value)) => {
                sort_binary_expr_metered!(
                    value,
                    EMinusMinusBody,
                    EMinusMinus,
                    Score::EMINUSMINUS,
                    meter
                )
            }
            Some(ExprInstance::EMatchesBody(value)) => {
                let target = ParSortMatcher::sort_match_metered(
                    value.target.as_ref().ok_or(BackingError::Rejected)?,
                    meter,
                )?;
                let pattern = ParSortMatcher::sort_match_metered(
                    value.pattern.as_ref().ok_or(BackingError::Rejected)?,
                    meter,
                )?;
                Ok(construct_metered(
                    ExprInstance::EMatchesBody(EMatches {
                        target: Some(target.term),
                        pattern: Some(pattern.term),
                    }),
                    Tree::<ScoreAtom>::create_node_from_i32_metered(
                        Score::EMATCHES,
                        score_children(target.score, pattern.score, meter)?,
                        meter,
                    )?,
                ))
            }
            Some(ExprInstance::EListBody(list)) => {
                let (ps, scores) = split_pars_metered(sort_pars_metered(&list.ps, meter)?, meter)?;
                Ok(construct_metered(
                    ExprInstance::EListBody(EList {
                        ps,
                        locally_free: meter.clone(&list.locally_free)?,
                        connective_used: list.connective_used,
                        remainder: meter.clone(&list.remainder)?,
                    }),
                    collection_score(
                        Score::ELIST,
                        &list.remainder,
                        scores,
                        list.connective_used,
                        meter,
                    )?,
                ))
            }
            Some(ExprInstance::EPathmapBody(pathmap)) => {
                let (canonical, scores) = sort_pathmap_metered(pathmap, meter, true)?;
                Ok(construct_metered(
                    ExprInstance::EPathmapBody(canonical),
                    collection_score(
                        Score::EPATHMAP,
                        &pathmap.remainder,
                        scores,
                        pathmap.connective_used,
                        meter,
                    )?,
                ))
            }
            Some(ExprInstance::EZipperBody(zipper)) => {
                let pathmap = zipper.pathmap.as_ref().ok_or(BackingError::Rejected)?;
                let (canonical, scores) = sort_pathmap_metered(pathmap, meter, false)?;
                let score = simple_collection_score(
                    Score::EPATHMAP + 1,
                    scores,
                    zipper.connective_used,
                    meter,
                )?;
                Ok(construct_metered(
                    ExprInstance::EZipperBody(EZipper {
                        pathmap: Some(canonical),
                        current_path: meter.clone(&zipper.current_path)?,
                        is_write_zipper: zipper.is_write_zipper,
                        locally_free: meter.clone(&zipper.locally_free)?,
                        connective_used: zipper.connective_used,
                        cursor_kind: zipper.cursor_kind,
                    }),
                    score,
                ))
            }
            Some(ExprInstance::ETupleBody(tuple)) => {
                let (ps, scores) = split_pars_metered(sort_pars_metered(&tuple.ps, meter)?, meter)?;
                let mut sorted_tuple = meter.clone(tuple)?;
                sorted_tuple.ps = ps;
                Ok(construct_metered(
                    ExprInstance::ETupleBody(sorted_tuple),
                    simple_collection_score(Score::ETUPLE, scores, tuple.connective_used, meter)?,
                ))
            }
            Some(ExprInstance::EMethodBody(method)) => {
                let (arguments, scores) =
                    split_pars_metered(sort_pars_metered(&method.arguments, meter)?, meter)?;
                let target = ParSortMatcher::sort_match_metered(
                    method.target.as_ref().ok_or(BackingError::Rejected)?,
                    meter,
                )?;
                let count = scores.len().checked_add(4).ok_or(BackingError::Overflow)?;
                let mut score_items = meter.vec(count)?;
                score_items.push(Tree::<ScoreAtom>::create_leaf_from_i64(
                    Score::EMETHOD as i64,
                ));
                score_items.push(Tree::<ScoreAtom>::create_leaf_from_string(
                    meter.clone(&method.method_name)?,
                ));
                score_items.push(target.score);
                score_items.extend(scores);
                score_items.push(Tree::<ScoreAtom>::create_leaf_from_i64(i64::from(
                    method.connective_used,
                )));
                let mut sorted_method = meter.clone(method)?;
                sorted_method.arguments = arguments;
                sorted_method.target = Some(target.term);
                Ok(construct_metered(
                    ExprInstance::EMethodBody(sorted_method),
                    Tree::Node(score_items),
                ))
            }
            Some(ExprInstance::ESetBody(set)) => Self::sort_set_metered(set, meter),
            Some(ExprInstance::EMapBody(map)) => Self::sort_map_metered(map, meter),
            Some(ExprInstance::GBool(value)) => Ok(ScoredTerm {
                term: meter.clone(expr)?,
                score: Tree::<ScoreAtom>::create_node_from_i64s_metered(
                    &[Score::BOOL as i64, i64::from(!*value)],
                    meter,
                )?,
            }),
            Some(ExprInstance::GInt(value)) => Ok(ScoredTerm {
                term: meter.clone(expr)?,
                score: Tree::<ScoreAtom>::create_node_from_i64s_metered(
                    &[Score::INT as i64, *value],
                    meter,
                )?,
            }),
            Some(ExprInstance::GString(value)) => Ok(ScoredTerm {
                term: meter.clone(expr)?,
                score: score_node(
                    Score::STRING,
                    Tree::<ScoreAtom>::create_leaf_from_string(meter.clone(value)?),
                    meter,
                )?,
            }),
            Some(ExprInstance::GUri(value)) => Ok(ScoredTerm {
                term: meter.clone(expr)?,
                score: score_node(
                    Score::URI,
                    Tree::<ScoreAtom>::create_leaf_from_string(meter.clone(value)?),
                    meter,
                )?,
            }),
            Some(ExprInstance::GByteArray(value)) => Ok(ScoredTerm {
                term: meter.clone(expr)?,
                score: score_node(
                    Score::EBYTEARR,
                    Tree::<ScoreAtom>::create_leaf_from_bytes(meter.clone(value)?),
                    meter,
                )?,
            }),
            Some(ExprInstance::GDouble(value)) => Ok(ScoredTerm {
                term: meter.clone(expr)?,
                score: Tree::<ScoreAtom>::create_node_from_i64s_metered(
                    &[Score::DOUBLE as i64, *value as i64],
                    meter,
                )?,
            }),
            Some(ExprInstance::GFloat32(value)) => Ok(ScoredTerm {
                term: meter.clone(expr)?,
                score: Tree::<ScoreAtom>::create_node_from_i64s_metered(
                    &[Score::FLOAT32 as i64, *value as i64],
                    meter,
                )?,
            }),
            Some(ExprInstance::GBigInt(value)) => Ok(ScoredTerm {
                term: meter.clone(expr)?,
                score: score_node(
                    Score::BIG_INT,
                    Tree::<ScoreAtom>::create_leaf_from_bytes(meter.clone(value)?),
                    meter,
                )?,
            }),
            Some(ExprInstance::GBigRat(value)) => Ok(ScoredTerm {
                term: meter.clone(expr)?,
                score: Tree::<ScoreAtom>::create_node_from_i32_metered(
                    Score::BIG_RAT,
                    score_children(
                        Tree::<ScoreAtom>::create_leaf_from_bytes(meter.clone(&value.numerator)?),
                        Tree::<ScoreAtom>::create_leaf_from_bytes(meter.clone(&value.denominator)?),
                        meter,
                    )?,
                    meter,
                )?,
            }),
            Some(ExprInstance::GFixedPoint(value)) => Ok(ScoredTerm {
                term: meter.clone(expr)?,
                score: Tree::<ScoreAtom>::create_node_from_i32_metered(
                    Score::FIXED_POINT,
                    score_children(
                        Tree::<ScoreAtom>::create_leaf_from_bytes(meter.clone(&value.unscaled)?),
                        Tree::<ScoreAtom>::create_node_from_i64s_metered(
                            &[value.scale as i64],
                            meter,
                        )?,
                        meter,
                    )?,
                    meter,
                )?,
            }),
            Some(ExprInstance::GUint64(value)) => Ok(ScoredTerm {
                term: meter.clone(expr)?,
                score: score_node(
                    Score::UINT64,
                    Tree::<ScoreAtom>::create_leaf_from_bytes(
                        meter.clone_slice(&value.to_be_bytes())?,
                    ),
                    meter,
                )?,
            }),
            Some(ExprInstance::GInt32(value)) => Ok(ScoredTerm {
                term: meter.clone(expr)?,
                score: Tree::<ScoreAtom>::create_node_from_i64s_metered(
                    &[Score::INT32 as i64, i64::from(*value)],
                    meter,
                )?,
            }),
            Some(ExprInstance::GUint32(value)) => Ok(ScoredTerm {
                term: meter.clone(expr)?,
                score: Tree::<ScoreAtom>::create_node_from_i64s_metered(
                    &[Score::UINT32 as i64, i64::from(*value)],
                    meter,
                )?,
            }),
            Some(ExprInstance::GUint16(value)) => Ok(ScoredTerm {
                term: meter.clone(expr)?,
                score: Tree::<ScoreAtom>::create_node_from_i64s_metered(
                    &[Score::UINT16 as i64, i64::from(*value)],
                    meter,
                )?,
            }),
            Some(ExprInstance::GUint8(value)) => Ok(ScoredTerm {
                term: meter.clone(expr)?,
                score: Tree::<ScoreAtom>::create_node_from_i64s_metered(
                    &[Score::UINT8 as i64, i64::from(*value)],
                    meter,
                )?,
            }),
            None => Ok(ScoredTerm {
                term: meter.clone(expr)?,
                score: Tree::<ScoreAtom>::create_node_from_i32_metered(
                    Score::ABSENT,
                    Vec::new(),
                    meter,
                )?,
            }),
        }
    }

    fn sort_set_metered(
        set: &crate::rhoapi::ESet,
        meter: &SorterMeter<'_>,
    ) -> Result<ScoredTerm<Expr>, BackingError> {
        let par_set = ParSetTypeMapper::eset_to_par_set_metered(meter.clone(set)?, meter)?;
        let (terms, scores) =
            split_pars_metered(sort_pars_metered(&par_set.ps.sorted_pars, meter)?, meter)?;
        let score = collection_score(
            Score::ESET,
            &par_set.remainder,
            scores,
            par_set.connective_used,
            meter,
        )?;
        let sorted = SortedParHashSet::create_from_vec_metered(terms, meter)?;
        Ok(construct_metered(
            ExprInstance::ESetBody(ParSetTypeMapper::par_set_to_eset_prepaid(ParSet {
                ps: sorted,
                connective_used: par_set.connective_used,
                locally_free: par_set.locally_free,
                remainder: par_set.remainder,
            })),
            score,
        ))
    }

    fn sort_map_metered(
        map: &crate::rhoapi::EMap,
        meter: &SorterMeter<'_>,
    ) -> Result<ScoredTerm<Expr>, BackingError> {
        let par_map = ParMapTypeMapper::emap_to_par_map_metered(meter.clone(map)?, meter)?;
        let mut terms = meter.vec(par_map.ps.sorted_list.len())?;
        let mut scores = meter.vec(par_map.ps.sorted_list.len())?;
        for (key, value) in &par_map.ps.sorted_list {
            let key = ParSortMatcher::sort_match_metered(key, meter)?;
            let value = ParSortMatcher::sort_match_metered(value, meter)?;
            terms.push((key.term, value.term));
            scores.push(key.score);
        }
        let score = collection_score(
            Score::EMAP,
            &par_map.remainder,
            scores,
            par_map.connective_used,
            meter,
        )?;
        let sorted = ParMap::new_metered(
            terms,
            par_map.connective_used,
            par_map.locally_free,
            par_map.remainder,
            meter,
        )?;
        Ok(construct_metered(
            ExprInstance::EMapBody(ParMapTypeMapper::par_map_to_emap_metered(sorted, meter)?),
            score,
        ))
    }
}

fn simple_collection_score(
    kind: i32,
    scores: Vec<Tree<ScoreAtom>>,
    connective_used: bool,
    meter: &SorterMeter<'_>,
) -> Result<Tree<ScoreAtom>, BackingError> {
    let count = scores.len().checked_add(2).ok_or(BackingError::Overflow)?;
    let mut items = meter.vec(count)?;
    items.push(Tree::<ScoreAtom>::create_leaf_from_i64(kind as i64));
    items.extend(scores);
    items.push(Tree::<ScoreAtom>::create_leaf_from_i64(i64::from(
        connective_used,
    )));
    Ok(Tree::Node(items))
}

#[cfg(test)]
mod metered_tests {
    use std::cell::RefCell;

    use super::*;
    use crate::rhoapi::{EMap, ESet, KeyValuePair};
    use crate::rust::utils::{new_gint_par, new_gstring_par};

    #[test]
    fn metered_expr_sort_preserves_recursive_and_collection_scores() {
        let left = new_gstring_par("left".to_owned(), Vec::new(), false);
        let right = new_gint_par(7, Vec::new(), false);
        let values = [
            Expr {
                expr_instance: Some(ExprInstance::GFloat32(1.5f32.to_bits())),
            },
            Expr {
                expr_instance: Some(ExprInstance::EPlusBody(EPlus {
                    p1: Some(left.clone()),
                    p2: Some(right.clone()),
                })),
            },
            Expr {
                expr_instance: Some(ExprInstance::EListBody(EList {
                    ps: vec![left.clone(), right.clone()],
                    locally_free: Vec::new(),
                    connective_used: false,
                    remainder: None,
                })),
            },
            Expr {
                expr_instance: Some(ExprInstance::ESetBody(ESet {
                    ps: vec![left.clone(), left.clone(), right.clone()],
                    locally_free: Vec::new(),
                    connective_used: false,
                    remainder: None,
                })),
            },
            Expr {
                expr_instance: Some(ExprInstance::EMapBody(EMap {
                    kvs: vec![
                        KeyValuePair {
                            key: Some(left.clone()),
                            value: Some(right.clone()),
                        },
                        KeyValuePair {
                            key: Some(left.clone()),
                            value: Some(left.clone()),
                        },
                    ],
                    locally_free: Vec::new(),
                    connective_used: false,
                    remainder: None,
                })),
            },
            Expr {
                expr_instance: Some(ExprInstance::EPathmapBody(EPathMap::new(
                    vec![left.clone(), right.clone()],
                    Vec::new(),
                    false,
                    None,
                ))),
            },
            Expr {
                expr_instance: Some(ExprInstance::EPathmapBody(EPathMap::new_map(
                    vec![(left.clone(), right.clone())],
                    Vec::new(),
                    false,
                    None,
                ))),
            },
            Expr {
                expr_instance: Some(ExprInstance::EZipperBody(EZipper {
                    pathmap: Some(EPathMap::new_map(
                        vec![(left.clone(), right.clone())],
                        Vec::new(),
                        false,
                        None,
                    )),
                    ..EZipper::default()
                })),
            },
        ];
        let reserve = |_: usize, _: usize, _: usize| Ok(());
        let meter = SorterMeter::new(&reserve);
        for value in values {
            assert_eq!(
                ExprSortMatcher::sort_match_metered(&value, &meter).unwrap(),
                ExprSortMatcher::sort_match(&value)
            );
        }
    }

    #[test]
    fn metered_expr_sort_matches_sized_integer_scores() {
        let values = [0, 1, u64::MAX]
            .map(ExprInstance::GUint64)
            .into_iter()
            .chain([i32::MIN, -1, 0, i32::MAX].map(ExprInstance::GInt32))
            .chain([0, 1, u32::MAX].map(ExprInstance::GUint32))
            .chain([0, u32::from(u16::MAX)].map(ExprInstance::GUint16))
            .chain([0, u32::from(u8::MAX)].map(ExprInstance::GUint8));
        let reserve = |_: usize, _: usize, _: usize| Ok(());
        let meter = SorterMeter::new(&reserve);
        for expr_instance in values {
            let value = Expr {
                expr_instance: Some(expr_instance),
            };
            assert_eq!(
                ExprSortMatcher::sort_match_metered(&value, &meter)
                    .expect("a sized integer sorts within an unbounded meter"),
                ExprSortMatcher::sort_match(&value)
            );
        }
    }

    #[test]
    fn metered_expr_sort_rejects_large_ground_before_copy() {
        let value = Expr {
            expr_instance: Some(ExprInstance::GString("payload".repeat(1024))),
        };
        let reserve = |_: usize, _: usize, backing: usize| {
            if backing >= 4096 {
                Err(BackingError::Rejected)
            } else {
                Ok(())
            }
        };
        let meter = SorterMeter::new(&reserve);
        assert!(matches!(
            ExprSortMatcher::sort_match_metered(&value, &meter),
            Err(BackingError::Rejected)
        ));
    }

    #[test]
    fn trie_map_tariff_is_deterministic_and_precedes_canonical_refile() {
        let key = new_gstring_par("key".to_owned(), Vec::new(), false);
        let value = new_gint_par(17, Vec::new(), false);
        let expr = Expr {
            expr_instance: Some(ExprInstance::EPathmapBody(EPathMap::new_map(
                vec![(key, value)],
                Vec::new(),
                false,
                None,
            ))),
        };
        let run = || {
            let trace = RefCell::new(Vec::new());
            let reserve = |ops, scanned, backing| {
                trace.borrow_mut().push([ops, scanned, backing]);
                Ok(())
            };
            let sorted = ExprSortMatcher::sort_match_metered(&expr, &SorterMeter::new(&reserve))
                .expect("metered trie sort");
            let charges = trace.into_inner();
            (sorted, charges)
        };
        let (first, trace) = run();
        let (second, replay) = run();
        assert_eq!(first, ExprSortMatcher::sort_match(&expr));
        assert_eq!(first, second);
        assert_eq!(trace, replay);
        assert!(trace
            .iter()
            .any(|&[ops, scanned, backing]| { ops == 2 && scanned == backing && backing >= 256 }));
    }

    #[test]
    fn malformed_nested_expr_fields_reject_without_panicking() {
        let values = [
            Expr {
                expr_instance: Some(ExprInstance::EPlusBody(EPlus {
                    p1: None,
                    p2: Some(Par::default()),
                })),
            },
            Expr {
                expr_instance: Some(ExprInstance::ENegBody(ENeg { p: None })),
            },
            Expr {
                expr_instance: Some(ExprInstance::EMatchesBody(EMatches {
                    target: Some(Par::default()),
                    pattern: None,
                })),
            },
        ];
        let reserve = |_: usize, _: usize, _: usize| Ok(());
        let meter = SorterMeter::new(&reserve);
        for value in values {
            assert!(matches!(
                ExprSortMatcher::sort_match_metered(&value, &meter),
                Err(BackingError::Rejected)
            ));
        }
    }
}
