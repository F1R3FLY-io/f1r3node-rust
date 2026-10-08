// See models/src/main/scala/coop/rchain/models/SortedParHashSet.scala

use std::collections::HashSet;

use shared::rust::clone_backing::{BackingError, BackingMeter};
use shared::rust::collection_backing::hash_backing;

use super::rholang::sorter::metered::SorterMeter;
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
    pub fn create_from_vec_metered(
        vec: Vec<Par>,
        backing: &dyn BackingMeter,
    ) -> Result<Self, BackingError> {
        let meter = SorterMeter::new(backing);
        let mut set = HashSet::new();
        let (operations, bytes) =
            hash_backing::<Par, ()>(vec.len()).ok_or(BackingError::Overflow)?;
        meter.reserve(operations, 0, bytes)?;
        set.try_reserve(vec.len())
            .map_err(|_| BackingError::Allocation)?;
        for par in vec {
            // Changed by D-O1 (DR-111): the insert hashes the value, then
            // compares it in lockstep or moves it (`hash_insert_blocks`).
            // meter.inspect(&par)?;
            meter.hash_insert_blocks(&par)?;
            set.insert(par);
        }
        let mut source = meter.vec(set.len())?;
        for par in &set {
            // Changed by D-O1 (DR-111): block accounting.
            // source.push(meter.clone(par)?);
            source.push(meter.clone_blocks(par)?);
        }
        let sorted_pars = Ordering::sort_pars_metered(&source, backing)?;
        let mut sorted_ps = HashSet::new();
        let (operations, bytes) =
            hash_backing::<Par, ()>(sorted_pars.len()).ok_or(BackingError::Overflow)?;
        meter.reserve(operations, 0, bytes)?;
        sorted_ps
            .try_reserve(sorted_pars.len())
            .map_err(|_| BackingError::Allocation)?;
        for par in &sorted_pars {
            // Changed by D-O1 (DR-111): the insert hashes the copy, then
            // compares it in lockstep or moves it (`hash_insert_blocks`). Two
            // distinct unsorted elements can sort equal.
            // meter.inspect(par)?;
            // sorted_ps.insert(meter.clone(par)?);
            meter.hash_insert_blocks(par)?;
            sorted_ps.insert(meter.clone_blocks(par)?);
        }
        Ok(Self {
            ps: set,
            sorted_pars,
            sorted_ps,
        })
    }

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
mod block_tests {
    use std::cell::RefCell;

    use shared::rust::clone_backing::{self, BackingError, BackingMeter};
    use shared::rust::collection_backing::hash_backing;

    use super::*;

    fn log_of(
        charge: impl FnOnce(&dyn BackingMeter) -> Result<(), BackingError>,
    ) -> Vec<[usize; 3]> {
        let log = RefCell::new(Vec::with_capacity(4_096));
        let backing = |operations: usize, scanned: usize, bytes: usize| {
            log.borrow_mut().push([operations, scanned, bytes]);
            Ok(())
        };
        charge(&backing).expect("an unlimited meter");
        log.into_inner()
    }

    /// D-E4 (DR-111): a metered set construction reserves its table, charges
    /// three block inspections for each insert (the hash, then a lockstep
    /// comparison or a move) and a block copy for each copied element. One
    /// distinct element given twice makes the second insert compare, and the
    /// sort of one element does not depend on the iteration order (pgmcp bug
    /// 475811 stays open for several distinct elements).
    #[test]
    fn set_construction_charges_hash_inserts_and_block_copies() {
        let par = crate::rust::utils::new_gstring_par("e".repeat(4_096), vec![1; 16], false);
        let log = log_of(|backing| {
            SortedParHashSet::create_from_vec_metered(vec![par.clone(), par.clone()], backing)
                .map(drop)
        });
        let insert = log_of(|meter| {
            for _ in 0..3 {
                clone_backing::inspect_blocks(&par, meter)?;
            }
            Ok(())
        });
        let copy = log_of(|meter| clone_backing::reserve_blocks_copy_and_cleanup(&par, meter));
        let table = |capacity: usize| {
            let (operations, bytes) = hash_backing::<Par, ()>(capacity).expect("a small table");
            vec![[operations, 0, bytes]]
        };
        let source = vec![par.clone()];
        let sort = log_of(|backing| Ordering::sort_pars_metered(&source, backing).map(drop));
        let expected = [
            table(2),
            insert.clone(),
            insert.clone(),
            vec![[1, 0, std::mem::size_of::<Par>()]],
            copy.clone(),
            sort,
            table(1),
            insert,
            copy,
        ]
        .concat();
        assert_eq!(log, expected);
    }
}
