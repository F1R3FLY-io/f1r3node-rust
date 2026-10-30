// See models/src/main/scala/coop/rchain/models/SortedParMap.scala

use std::collections::HashMap;

use super::rholang::sorter::ordering::Ordering;
use super::rholang::sorter::par_sort_matcher::ParSortMatcher;
use super::rholang::sorter::sortable::Sortable;
use crate::rhoapi::Par;

#[derive(Clone, Debug)]
pub struct SortedParMap {
    pub ps: HashMap<Par, Par>,
    // TODO: Merge `sortedList` and `sortedMap` into one VectorMap once available - OLD
    pub sorted_list: Vec<(Par, Par)>,
    sorted_map: HashMap<Par, Par>,
}

impl SortedParMap {
    pub fn create_from_map(map: HashMap<Par, Par>) -> Self {
        let sorted_list = Ordering::sort_map(&map);
        let sorted_map = sorted_list.clone().into_iter().collect();

        SortedParMap {
            ps: map,
            sorted_list,
            sorted_map,
        }
    }

    pub fn create_from_vec(vec: Vec<(Par, Par)>) -> Self {
        let map: HashMap<Par, Par> = vec.into_iter().collect();
        SortedParMap::create_from_map(map)
    }

    pub fn create_from_empty() -> Self { SortedParMap::create_from_map(HashMap::new()) }

    pub fn map_iter<'a, F, T>(&'a self, f: F) -> impl Iterator<Item = T> + 'a
    where F: Fn((&Par, &Par)) -> T + 'a {
        self.sorted_list.iter().map(move |(k, v)| f((k, v)))
    }

    // alias for '+'
    pub fn insert(&mut self, kv: (Par, Par)) -> SortedParMap {
        self.sorted_map.insert(kv.0, kv.1);
        Self::create_from_map(self.sorted_map.clone())
    }

    // alias for '++'
    pub fn extend(&mut self, kvs: Vec<(Par, Par)>) -> SortedParMap {
        self.sorted_map.extend(kvs);
        Self::create_from_map(self.sorted_map.clone())
    }

    // alias for '-'
    pub fn remove(&mut self, key: Par) -> SortedParMap {
        self.sorted_map.remove(&Self::sort(&key));
        Self::create_from_map(self.sorted_map.clone())
    }

    // alias for '--'
    pub fn remove_multiple(&mut self, keys: Vec<Par>) -> SortedParMap {
        for key in keys {
            self.sorted_map.remove(&Self::sort(&key));
        }
        Self::create_from_map(self.sorted_map.clone())
    }

    pub fn apply(&self, key: Par) -> Option<Par> { self.sorted_map.get(&Self::sort(&key)).cloned() }

    pub fn contains(&self, par: Par) -> bool {
        self.sorted_map.contains_key(&SortedParMap::sort(&par))
    }

    pub fn get(&self, key: Par) -> Option<Par> {
        self.sorted_map.get(&SortedParMap::sort(&key)).cloned()
    }

    pub fn get_or_else(&self, key: Par, default: Par) -> Par {
        match self.sorted_map.get(&SortedParMap::sort(&key)) {
            Some(value) => value.clone(),
            None => default,
        }
    }

    pub fn keys(&self) -> Vec<Par> {
        self.sorted_list
            .clone()
            .into_iter()
            .map(|kv| kv.0)
            .collect()
    }

    pub fn values(&self) -> Vec<Par> {
        self.sorted_list
            .clone()
            .into_iter()
            .map(|kv| kv.1)
            .collect()
    }

    pub fn equals(&self, that: SortedParMap) -> bool { self.sorted_list == that.sorted_list }

    pub fn length(&self) -> usize { self.sorted_list.len() }

    pub fn is_empty(&self) -> bool {
        self.ps.is_empty() && self.sorted_list.is_empty() && self.sorted_map.is_empty()
    }

    fn sort(par: &Par) -> Par { ParSortMatcher::sort_match(par).term.clone() }
}

impl IntoIterator for SortedParMap {
    type Item = (Par, Par);
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter { self.sorted_list.into_iter() }
}

impl PartialEq for SortedParMap {
    fn eq(&self, other: &Self) -> bool { self.sorted_list == other.sorted_list }
}

#[cfg(test)]
mod extend_differential_tests {
    use proptest::prelude::*;
    use prost::Message;

    use super::*;
    use crate::rhoapi::expr::ExprInstance;
    use crate::rhoapi::{EList, ETuple, Expr};

    fn legacy_extend(map: &mut SortedParMap, kvs: Vec<(Par, Par)>) -> SortedParMap {
        for kv in kvs {
            map.insert(kv);
        }
        SortedParMap::create_from_map(map.sorted_map.clone())
    }

    fn leaf() -> impl Strategy<Value = Expr> {
        prop_oneof![
            (0i64..4).prop_map(|n| Expr {
                expr_instance: Some(ExprInstance::GInt(n))
            }),
            prop::sample::select(vec!["a", "b", "c"]).prop_map(|s| Expr {
                expr_instance: Some(ExprInstance::GString(s.to_string())),
            }),
            any::<bool>().prop_map(|b| Expr {
                expr_instance: Some(ExprInstance::GBool(b))
            }),
        ]
    }

    fn expr() -> impl Strategy<Value = Expr> {
        prop_oneof![
            3 => leaf(),
            1 => prop::collection::vec(leaf(), 1..3).prop_map(|es| Expr {
                expr_instance: Some(ExprInstance::ETupleBody(ETuple {
                    ps: es.into_iter().map(|e| Par::default().with_exprs(vec![e])).collect(),
                    ..Default::default()
                })),
            }),
            1 => prop::collection::vec(leaf(), 0..3).prop_map(|es| Expr {
                expr_instance: Some(ExprInstance::EListBody(EList {
                    ps: es.into_iter().map(|e| Par::default().with_exprs(vec![e])).collect(),
                    ..Default::default()
                })),
            }),
        ]
    }

    fn par() -> impl Strategy<Value = Par> {
        prop::collection::vec(expr(), 1..3).prop_map(|es| Par::default().with_exprs(es))
    }

    fn pairs(max: usize) -> impl Strategy<Value = Vec<(Par, Par)>> {
        prop::collection::vec((par(), par()), 0..max)
    }

    fn bytes(map: &SortedParMap) -> Vec<(Vec<u8>, Vec<u8>)> {
        map.sorted_list
            .iter()
            .map(|(k, v)| (k.encode_to_vec(), v.encode_to_vec()))
            .collect()
    }

    fn canonical_keys_are_distinct(map: &SortedParMap) -> bool {
        let canonical: std::collections::HashSet<Vec<u8>> = map
            .sorted_map
            .keys()
            .map(|k| SortedParMap::sort(k).encode_to_vec())
            .collect();
        canonical.len() == map.sorted_map.len()
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            cases: 512,
            max_global_rejects: 100_000,
            ..ProptestConfig::default()
        })]

        #[test]
        fn extend_matches_the_per_element_implementation(
            initial in pairs(10),
            kvs in pairs(40),
            repeats in prop::collection::vec((any::<prop::sample::Index>(), par()), 0..20),
        ) {
            let mut kvs = kvs;
            if !kvs.is_empty() {
                let again: Vec<(Par, Par)> =
                    repeats.into_iter().map(|(i, v)| (i.get(&kvs).0.clone(), v)).collect();
                kvs.extend(again);
            }

            let mut legacy = SortedParMap::create_from_vec(initial);
            let mut fixed = legacy.clone();
            let expected = legacy_extend(&mut legacy, kvs.clone());
            prop_assume!(canonical_keys_are_distinct(&legacy));
            let actual = fixed.extend(kvs);
            prop_assert_eq!(bytes(&actual), bytes(&expected));
            prop_assert_eq!(&actual.ps, &expected.ps);
            prop_assert_eq!(&actual.sorted_map, &expected.sorted_map);
            prop_assert_eq!(&fixed.sorted_map, &legacy.sorted_map);
            prop_assert_eq!(bytes(&fixed), bytes(&legacy));
            prop_assert_eq!(&fixed.ps, &legacy.ps);
        }
    }

    #[test]
    #[ignore = "microbenchmark: run with --release --ignored --nocapture"]
    fn extend_microbenchmark() {
        let int = |n: i64| {
            Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::GInt(n)),
            }])
        };
        for n in [250i64, 1_000, 4_000] {
            let kvs: Vec<(Par, Par)> = (0..n).map(|i| (int(i), int(-i))).collect();
            let started = std::time::Instant::now();
            let expected = legacy_extend(&mut SortedParMap::create_from_empty(), kvs.clone());
            let legacy_time = started.elapsed();
            let started = std::time::Instant::now();
            let actual = SortedParMap::create_from_empty().extend(kvs);
            let fixed_time = started.elapsed();
            assert_eq!(bytes(&actual), bytes(&expected));
            println!(
                "SortedParMap::extend, {n} pairs: per-element {:?}, once {:?}, {:.0}x",
                legacy_time,
                fixed_time,
                legacy_time.as_secs_f64() / fixed_time.as_secs_f64()
            );
        }
    }

    #[test]
    fn a_duplicate_key_keeps_the_last_value() {
        let k = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(1)),
        }]);
        let v = |n| {
            Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::GInt(n)),
            }])
        };
        let mut legacy = SortedParMap::create_from_empty();
        let mut fixed = SortedParMap::create_from_empty();
        let kvs = vec![(k.clone(), v(1)), (k.clone(), v(2)), (k.clone(), v(3))];
        let expected = legacy_extend(&mut legacy, kvs.clone());
        let actual = fixed.extend(kvs);
        assert_eq!(bytes(&actual), bytes(&expected));
        assert_eq!(actual.get(k), Some(v(3)));
    }
}
