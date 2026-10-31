// See models/src/main/scala/coop/rchain/models/SortedParHashSet.scala

use std::collections::HashSet;

use super::rholang::sorter::ordering::Ordering;
use super::rholang::sorter::par_sort_matcher::ParSortMatcher;
use super::rholang::sorter::sortable::Sortable;
use crate::rhoapi::Par;

// Enforce ordering and uniqueness.
// - uniqueness is handled by using HashSet.
// - ordering comes from sorting the elements prior to serializing.
#[derive(Clone)]
pub struct SortedParHashSet {
    pub ps: HashSet<Par>,
    pub sorted_pars: Vec<Par>,
    pub sorted_ps: HashSet<Par>,
}

impl SortedParHashSet {
    pub fn create_from_vec(vec: Vec<Par>) -> Self {
        let set: HashSet<Par> = vec.clone().into_iter().collect();
        let sorted_pars = Ordering::sort_pars(&set.clone().into_iter().collect());
        let sorted_ps: HashSet<Par> = sorted_pars.clone().into_iter().collect();

        SortedParHashSet {
            ps: set,
            sorted_pars,
            sorted_ps,
        }
    }

    pub fn create_from_set(set: HashSet<Par>) -> Self {
        let vec = set.into_iter().collect();
        SortedParHashSet::create_from_vec(vec)
    }

    pub fn create_from_empty() -> Self { SortedParHashSet::create_from_set(HashSet::new()) }

    pub fn map_iter<'a, F, T>(&'a self, f: F) -> impl Iterator<Item = T> + 'a
    where F: Fn(&Par) -> T + 'a {
        self.sorted_pars.iter().map(f)
    }

    // alias for '+'
    pub fn insert(&mut self, elem: Par) -> SortedParHashSet {
        self.ps.insert(Self::sort(&elem));
        Self::create_from_set(self.ps.clone())
    }

    // alias for '-'
    pub fn remove(&mut self, elem: Par) -> SortedParHashSet {
        self.ps.remove(&Self::sort(&elem));
        Self::create_from_set(self.ps.clone())
    }

    pub fn contains(&self, elem: Par) -> bool { self.sorted_ps.contains(&Self::sort(&elem)) }

    pub fn union(&self, that: HashSet<Par>) -> SortedParHashSet {
        SortedParHashSet::create_from_set(
            self.sorted_ps
                .union(&that.iter().map(Self::sort).collect())
                .cloned()
                .collect(),
        )
    }

    pub fn equals(&self, that: SortedParHashSet) -> bool { self.sorted_pars == that.sorted_pars }

    pub fn length(&self) -> usize { self.sorted_ps.len() }

    pub fn sort(par: &Par) -> Par { ParSortMatcher::sort_match(par).term.clone() }
}

impl PartialEq for SortedParHashSet {
    fn eq(&self, other: &Self) -> bool { self.sorted_pars == other.sorted_pars }
}

use std::fmt;

impl fmt::Debug for SortedParHashSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SortedParHashSet")
            .field("ps", &self.ps)
            .field("sorted_pars", &self.sorted_pars)
            .field("sorted_ps", &self.sorted_ps)
            .finish()
    }
}

#[cfg(test)]
mod tie_tests {
    use proptest::prelude::*;
    use prost::Message;

    use super::*;
    use crate::rhoapi::expr::ExprInstance;
    use crate::rhoapi::Expr;

    fn e(instance: ExprInstance) -> Expr {
        Expr {
            expr_instance: Some(instance),
        }
    }

    fn output(elements: &[Par], rotation: usize) -> Vec<Vec<u8>> {
        let mut shuffled = elements.to_vec();
        if !shuffled.is_empty() {
            let n = rotation % shuffled.len();
            shuffled.rotate_left(n);
        }
        SortedParHashSet::create_from_vec(shuffled)
            .sorted_pars
            .iter()
            .map(|p| p.encode_to_vec())
            .collect()
    }

    #[test]
    fn a_fixed_collision_sorts_the_same_in_200_runs() {
        let k1 = Par::default()
            .with_exprs(vec![
                e(ExprInstance::GInt(1)),
                e(ExprInstance::GString("a".into())),
            ])
            .with_locally_free(vec![1]);
        let k2 = Par::default()
            .with_exprs(vec![
                e(ExprInstance::GString("a".into())),
                e(ExprInstance::GInt(1)),
            ])
            .with_locally_free(vec![2]);
        let seen: std::collections::HashSet<_> = (0..200)
            .map(|run| output(&[k1.clone(), k2.clone()], run))
            .collect();
        assert_eq!(seen.len(), 1);
    }

    fn leaf() -> impl Strategy<Value = Expr> {
        prop_oneof![
            (0i64..4).prop_map(|n| e(ExprInstance::GInt(n))),
            prop::sample::select(vec!["a", "b", "c"])
                .prop_map(|s| e(ExprInstance::GString(s.to_string()))),
            any::<bool>().prop_map(|b| e(ExprInstance::GBool(b))),
        ]
    }

    fn colliding_elements() -> impl Strategy<Value = Vec<Par>> {
        prop::collection::vec((leaf(), leaf(), 0u8..4, 0u8..4), 1..10).prop_map(|quads| {
            quads
                .into_iter()
                .flat_map(|(a, b, fa, fb)| {
                    let ab = Par::default()
                        .with_exprs(vec![a.clone(), b.clone()])
                        .with_locally_free(vec![fa]);
                    let ba = Par::default()
                        .with_exprs(vec![b, a])
                        .with_locally_free(vec![fb]);
                    [ab, ba]
                })
                .fold(Vec::new(), |mut acc: Vec<Par>, p| {
                    if !acc.contains(&p) {
                        acc.push(p);
                    }
                    acc
                })
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn canonically_equal_elements_sort_the_same_in_every_run(
            elements in colliding_elements()
        ) {
            let first = output(&elements, 0);
            for run in 1..200 {
                prop_assert_eq!(&output(&elements, run), &first, "run {}", run);
            }
        }
    }
}
