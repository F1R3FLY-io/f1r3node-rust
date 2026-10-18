// See rholang/src/main/scala/coop/rchain/rholang/interpreter/compiler/ReceiveBindsSortMatcher.scala

use models::rhoapi::{Par, ReceiveBind, Var};
use models::rust::par_construction::{ParConstruction, PlannedPar};
use models::rust::rholang::sorter::receive_sort_matcher::ReceiveSortMatcher;
use models::rust::rholang::sorter::score_tree::ScoredTerm;

use crate::rust::interpreter::compiler::exports::FreeMap;
use crate::rust::interpreter::compiler::normalize_drive::PlannedReceiveBind;
use crate::rust::interpreter::errors::InterpreterError;

pub fn pre_sort_binds<T: Clone + std::fmt::Debug>(
    binds: Vec<(Vec<Par>, Option<Var>, Par, FreeMap<T>)>,
) -> Result<Vec<(ReceiveBind, FreeMap<T>)>, InterpreterError> {
    let mut bind_sortings: Vec<ScoredTerm<(ReceiveBind, FreeMap<T>)>> = binds
        .into_iter()
        .map(|(patterns, remainder, channel, known_free)| {
            let sorted_bind = ReceiveSortMatcher::sort_bind(ReceiveBind {
                patterns,
                source: Some(channel),
                remainder,
                free_count: known_free.count_no_wildcards() as i32,
                cost_region: None,
            });

            ScoredTerm {
                term: (sorted_bind.term, known_free),
                score: sorted_bind.score,
            }
        })
        .collect();

    ScoredTerm::sort_vec(&mut bind_sortings);
    Ok(bind_sortings
        .into_iter()
        .map(|scored_term| scored_term.term)
        .collect())
}

/// Planned-arena equivalent of [`pre_sort_binds`]. Canonical score comparison
/// and protobuf tie-breaking are both cursor-based; no pattern or channel is
/// finished into a resident `Par` for ordering.
pub(crate) fn pre_sort_planned_binds<T: Clone + std::fmt::Debug>(
    arena: &mut ParConstruction,
    binds: Vec<(
        Vec<PlannedPar>,
        Option<Var>,
        PlannedPar,
        FreeMap<T>,
        Option<models::rhoapi::CostRegion>,
    )>,
) -> Result<Vec<(PlannedReceiveBind, FreeMap<T>)>, InterpreterError> {
    let mut planned = Vec::with_capacity(binds.len());
    for (patterns, remainder, source, known_free, cost_region) in binds {
        let free_count = i32::try_from(known_free.count_no_wildcards()).map_err(|_| {
            InterpreterError::NormalizerError(
                "receive-bind free count exceeds the protobuf i32 domain".to_string(),
            )
        })?;
        let shell = ReceiveBind {
            patterns: vec![Par::default(); patterns.len()],
            source: Some(Par::default()),
            remainder: remainder.clone(),
            free_count,
            cost_region: cost_region.clone(),
        };
        let order = arena.receive_bind_order(&shell, &patterns, source)?;
        planned.push((
            order,
            PlannedReceiveBind {
                patterns,
                source,
                remainder,
                free_count,
                cost_region,
            },
            known_free,
        ));
    }
    planned.sort_by(|left, right| arena.compare_receive_bind_orders(left.0, right.0));
    Ok(planned
        .into_iter()
        .map(|(_, bind, free_map)| (bind, free_map))
        .collect())
}

#[cfg(test)]
mod tests {
    use models::rust::utils::{new_freevar_var, new_gint_par};

    use super::*;
    use crate::rust::interpreter::compiler::normalize::VarSort;

    #[test]
    fn binds_should_pre_sort_based_on_their_channel_and_then_patterns() {
        let empty_map = FreeMap::new();

        let binds: Vec<(Vec<Par>, Option<Var>, Par, FreeMap<VarSort>)> = vec![
            (
                vec![new_gint_par(2, Vec::new(), false)],
                None,
                new_gint_par(3, Vec::new(), false),
                empty_map.clone(),
            ),
            (
                vec![new_gint_par(3, Vec::new(), false)],
                None,
                new_gint_par(2, Vec::new(), false),
                empty_map.clone(),
            ),
            (
                vec![new_gint_par(3, Vec::new(), false)],
                Some(new_freevar_var(0)),
                new_gint_par(2, Vec::new(), false),
                empty_map.clone(),
            ),
            (
                vec![new_gint_par(1, Vec::new(), false)],
                None,
                new_gint_par(3, Vec::new(), false),
                empty_map.clone(),
            ),
        ];

        let sorted_binds: Vec<(ReceiveBind, FreeMap<VarSort>)> = vec![
            (
                ReceiveBind {
                    patterns: vec![new_gint_par(3, Vec::new(), false)],
                    source: Some(new_gint_par(2, Vec::new(), false)),
                    remainder: None,
                    free_count: 0,
                    cost_region: None,
                },
                empty_map.clone(),
            ),
            (
                ReceiveBind {
                    patterns: vec![new_gint_par(3, Vec::new(), false)],
                    source: Some(new_gint_par(2, Vec::new(), false)),
                    remainder: Some(new_freevar_var(0)),
                    free_count: 0,
                    cost_region: None,
                },
                empty_map.clone(),
            ),
            (
                ReceiveBind {
                    patterns: vec![new_gint_par(1, Vec::new(), false)],
                    source: Some(new_gint_par(3, Vec::new(), false)),
                    remainder: None,
                    free_count: 0,
                    cost_region: None,
                },
                empty_map.clone(),
            ),
            (
                ReceiveBind {
                    patterns: vec![new_gint_par(2, Vec::new(), false)],
                    source: Some(new_gint_par(3, Vec::new(), false)),
                    remainder: None,
                    free_count: 0,
                    cost_region: None,
                },
                empty_map,
            ),
        ];

        let result = pre_sort_binds(binds);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), sorted_binds);
    }
}
