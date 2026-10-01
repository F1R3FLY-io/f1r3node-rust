// See models/src/main/scala/coop/rchain/models/rholang/sorter/ordering.scala

use std::collections::HashMap;

use shared::rust::clone_backing::{BackingError, BackingMeter};

use super::metered::SorterMeter;
use super::par_sort_matcher::ParSortMatcher;
use super::score_tree::ScoredTerm;
use super::sortable::Sortable;
use crate::rhoapi::Par;

pub struct Ordering;

impl Ordering {
    pub fn sort_pars_metered(
        ps: &[Par],
        backing: &dyn BackingMeter,
    ) -> Result<Vec<Par>, BackingError> {
        let meter = SorterMeter::new(backing);
        let mut scored = meter.vec(ps.len())?;
        for par in ps {
            scored.push(ParSortMatcher::sort_match_metered(par, &meter)?);
        }
        ScoredTerm::sort_vec_metered(&mut scored, &meter)?;
        let mut sorted = meter.vec(scored.len())?;
        sorted.extend(scored.into_iter().map(|item| item.term));
        Ok(sorted)
    }

    pub fn sort_map_metered(
        ps: &HashMap<Par, Par>,
        backing: &dyn BackingMeter,
    ) -> Result<Vec<(Par, Par)>, BackingError> {
        let meter = SorterMeter::new(backing);
        let mut scored = meter.vec(ps.len())?;
        for (key, value) in ps {
            let sorted_key = ParSortMatcher::sort_match_metered(key, &meter)?;
            let sorted_value = ParSortMatcher::sort_match_metered(value, &meter)?;
            scored.push(ScoredTerm {
                term: (sorted_key.term, sorted_value.term),
                score: sorted_key.score,
            });
        }
        ScoredTerm::sort_vec_metered(&mut scored, &meter)?;
        let mut sorted = meter.vec(scored.len())?;
        sorted.extend(scored.into_iter().map(|item| item.term));
        Ok(sorted)
    }

    pub fn sort_pars(ps: &Vec<Par>) -> Vec<Par> {
        let mut ps_sorted: Vec<ScoredTerm<Par>> =
            ps.iter().map(ParSortMatcher::sort_match).collect();
        ScoredTerm::sort_vec(&mut ps_sorted);
        ps_sorted.into_iter().map(|st| st.term).collect()
    }

    pub fn sort_key_value_pair(key: &Par, value: &Par) -> ScoredTerm<(Par, Par)> {
        let sorted_key = ParSortMatcher::sort_match(key);
        let sorted_value = ParSortMatcher::sort_match(value);

        ScoredTerm {
            term: (sorted_key.term, sorted_value.term),
            score: sorted_key.score,
        }
    }

    pub fn sort_map(ps: &HashMap<Par, Par>) -> Vec<(Par, Par)> {
        let mut pairs_sorted: Vec<ScoredTerm<(Par, Par)>> = ps
            .iter()
            .map(|kv| Ordering::sort_key_value_pair(kv.0, kv.1))
            .collect();

        ScoredTerm::sort_vec(&mut pairs_sorted);
        pairs_sorted.into_iter().map(|st| st.term).collect()
    }
}

#[cfg(test)]
mod metered_tests {
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use super::*;
    use crate::rhoapi::connective::ConnectiveInstance;
    use crate::rhoapi::expr::ExprInstance;
    use crate::rhoapi::g_unforgeable::UnfInstance;
    use crate::rhoapi::{
        Bundle, Connective, ConnectiveBody, EMap, ESet, Expr, GPrivate, GUnforgeable, If,
        KeyValuePair, Match, MatchCase, New, Receive, ReceiveBind, Send,
    };
    use crate::rust::utils::new_gstring_expr;

    fn string_par(value: &str) -> Par {
        Par {
            exprs: vec![new_gstring_expr(value.to_owned())],
            ..Par::default()
        }
    }

    #[test]
    fn nested_set_and_map_keep_legacy_canonical_order() {
        let first = string_par("first");
        let second = string_par("second");
        let nested_set = Par {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::ESetBody(ESet {
                    ps: vec![second.clone(), first.clone()],
                    ..ESet::default()
                })),
            }],
            ..Par::default()
        };
        let nested_map = Par {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::EMapBody(EMap {
                    kvs: vec![KeyValuePair {
                        key: Some(second.clone()),
                        value: Some(first.clone()),
                    }],
                    ..EMap::default()
                })),
            }],
            ..Par::default()
        };
        let input = vec![nested_map, second, nested_set, first];
        let meter = |_: usize, _: usize, _: usize| Ok(());
        assert_eq!(
            Ordering::sort_pars_metered(&input, &meter).unwrap(),
            Ordering::sort_pars(&input)
        );
    }

    #[test]
    fn recursive_variant_families_keep_legacy_term_and_score() {
        let leaf = string_par("leaf");
        let value = Par {
            sends: vec![Send {
                chan: Some(leaf.clone()),
                data: vec![leaf.clone()],
                ..Send::default()
            }],
            receives: vec![Receive {
                binds: vec![ReceiveBind {
                    patterns: vec![leaf.clone()],
                    source: Some(leaf.clone()),
                    ..ReceiveBind::default()
                }],
                body: Some(leaf.clone()),
                condition: Some(leaf.clone()),
                ..Receive::default()
            }],
            news: vec![New {
                p: Some(leaf.clone()),
                uri: vec!["z".to_owned(), "a".to_owned()],
                injections: BTreeMap::from([("injected".to_owned(), leaf.clone())]),
                ..New::default()
            }],
            matches: vec![Match {
                target: Some(leaf.clone()),
                cases: vec![MatchCase {
                    pattern: Some(leaf.clone()),
                    source: Some(leaf.clone()),
                    guard: Some(leaf.clone()),
                    ..MatchCase::default()
                }],
                ..Match::default()
            }],
            bundles: vec![Bundle {
                body: Some(leaf.clone()),
                read_flag: true,
                ..Bundle::default()
            }],
            conditionals: vec![If {
                condition: Some(leaf.clone()),
                if_true: Some(leaf.clone()),
                if_false: Some(leaf.clone()),
                ..If::default()
            }],
            connectives: vec![Connective {
                connective_instance: Some(ConnectiveInstance::ConnAndBody(ConnectiveBody {
                    ps: vec![leaf.clone(), Par::default()],
                })),
            }],
            unforgeables: vec![GUnforgeable {
                unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id: vec![1, 2, 3] })),
            }],
            ..Par::default()
        };
        let legacy = ParSortMatcher::sort_match(&value);
        let backing = |_: usize, _: usize, _: usize| Ok(());
        let metered =
            ParSortMatcher::sort_match_metered(&value, &SorterMeter::new(&backing)).unwrap();
        assert_eq!(metered, legacy);
    }

    #[test]
    fn large_score_payload_rejects_before_sort_output() {
        let input = vec![string_par(&"payload".repeat(2048))];
        let spent = Mutex::new(0usize);
        let meter = |_: usize, _: usize, backing: usize| {
            let mut used = spent.lock().unwrap();
            let next = used.checked_add(backing).ok_or(BackingError::Overflow)?;
            if next > 1024 {
                return Err(BackingError::Rejected);
            }
            *used = next;
            Ok(())
        };
        assert!(matches!(
            Ordering::sort_pars_metered(&input, &meter),
            Err(BackingError::Rejected)
        ));
    }

    #[test]
    fn malformed_send_rejects_without_panicking() {
        let input = vec![Par {
            sends: vec![Send::default()],
            ..Par::default()
        }];
        let meter = |_: usize, _: usize, _: usize| Ok(());
        assert!(matches!(
            Ordering::sort_pars_metered(&input, &meter),
            Err(BackingError::Rejected)
        ));
    }
}
