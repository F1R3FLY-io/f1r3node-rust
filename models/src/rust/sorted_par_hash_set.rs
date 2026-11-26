// See models/src/main/scala/coop/rchain/models/SortedParHashSet.scala

use std::collections::HashSet;

use super::rholang::sorter::ordering::Ordering;
use crate::rhoapi::Par;

// Enforce ordering and uniqueness.
// - uniqueness is handled by using HashSet.
// - ordering comes from sorting the elements prior to serializing.
#[derive(Clone)]
pub struct SortedParHashSet {
    pub ps: HashSet<Par>,
    pub sorted_pars: Vec<Par>,
}

impl SortedParHashSet {
    pub fn create_from_vec(vec: Vec<Par>) -> Self {
        let set: HashSet<Par> = vec.into_iter().collect();
        let sorted_pars = Ordering::sort_pars(&set);

        SortedParHashSet {
            ps: set,
            sorted_pars,
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

    pub fn equals(&self, that: SortedParHashSet) -> bool { self.sorted_pars == that.sorted_pars }
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
            .finish()
    }
}
