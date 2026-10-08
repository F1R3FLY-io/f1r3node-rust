// See models/src/main/scala/coop/rchain/models/SortedParMap.scala

use std::collections::HashMap;

use shared::rust::clone_backing::{BackingError, BackingMeter};
use shared::rust::collection_backing::hash_backing;

use super::rholang::sorter::metered::SorterMeter;
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
    pub fn into_sorted_list_prepaid(self) -> Vec<(Par, Par)> { self.sorted_list }

    pub fn create_from_vec_metered(
        vec: Vec<(Par, Par)>,
        backing: &dyn BackingMeter,
    ) -> Result<Self, BackingError> {
        let meter = SorterMeter::new(backing);
        let mut map = HashMap::new();
        let (operations, bytes) =
            hash_backing::<Par, Par>(vec.len()).ok_or(BackingError::Overflow)?;
        meter.reserve(operations, 0, bytes)?;
        map.try_reserve(vec.len())
            .map_err(|_| BackingError::Allocation)?;
        for (key, value) in vec {
            // Changed by D-O1 (DR-111): the insert hashes the key, then
            // compares it in lockstep or moves it (`hash_insert_blocks`). The
            // value moves in, or moves out of the table when a key repeats.
            // meter.inspect(&key)?;
            // meter.inspect(&value)?;
            meter.hash_insert_blocks(&key)?;
            meter.inspect_blocks(&value)?;
            map.insert(key, value);
        }
        let sorted_list = Ordering::sort_map_metered(&map, backing)?;
        let mut sorted_map = HashMap::new();
        let (operations, bytes) =
            hash_backing::<Par, Par>(sorted_list.len()).ok_or(BackingError::Overflow)?;
        meter.reserve(operations, 0, bytes)?;
        sorted_map
            .try_reserve(sorted_list.len())
            .map_err(|_| BackingError::Allocation)?;
        for (key, value) in &sorted_list {
            // Changed by D-O1 (DR-111): the insert hashes the copy of the key,
            // then compares it in lockstep or moves it (`hash_insert_blocks`).
            // meter.inspect(key)?;
            // sorted_map.insert(meter.clone(key)?, meter.clone(value)?);
            meter.hash_insert_blocks(key)?;
            sorted_map.insert(meter.clone_blocks(key)?, meter.clone_blocks(value)?);
        }
        Ok(Self {
            ps: map,
            sorted_list,
            sorted_map,
        })
    }

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
        for kv in kvs {
            self.insert(kv);
        }
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

    /// D-E4 (DR-111): a metered map construction reserves its table and
    /// charges three block inspections for each key insert and one for each
    /// value, which moves in or moves out when its key repeats. Each copied
    /// entry gets two block copies. One distinct key given twice makes the
    /// second insert compare and replace (pgmcp bug 475811 stays open for
    /// several distinct keys).
    #[test]
    fn map_construction_charges_hash_inserts_and_block_copies() {
        let key = crate::rust::utils::new_gstring_par("k".repeat(4_096), vec![1; 16], false);
        let first = crate::rust::utils::new_gstring_par("v".repeat(64), Vec::new(), false);
        let second = crate::rust::utils::new_gstring_par("w".repeat(4_096), Vec::new(), false);
        let log = log_of(|backing| {
            SortedParMap::create_from_vec_metered(
                vec![(key.clone(), first.clone()), (key.clone(), second.clone())],
                backing,
            )
            .map(drop)
        });
        let insert = log_of(|meter| {
            for _ in 0..3 {
                clone_backing::inspect_blocks(&key, meter)?;
            }
            Ok(())
        });
        let table = |capacity: usize| {
            let (operations, bytes) = hash_backing::<Par, Par>(capacity).expect("a small table");
            vec![[operations, 0, bytes]]
        };
        let map = HashMap::from([(key.clone(), second.clone())]);
        let sort = log_of(|backing| Ordering::sort_map_metered(&map, backing).map(drop));
        let sorted = Ordering::sort_map_metered(&map, &|_: usize, _: usize, _: usize| Ok(()))
            .expect("an unlimited sort");
        let (sorted_key, sorted_value) = &sorted[0];
        let expected = [
            table(2),
            insert.clone(),
            log_of(|meter| clone_backing::inspect_blocks(&first, meter)),
            insert.clone(),
            log_of(|meter| clone_backing::inspect_blocks(&second, meter)),
            sort,
            table(1),
            log_of(|meter| {
                for _ in 0..3 {
                    clone_backing::inspect_blocks(sorted_key, meter)?;
                }
                Ok(())
            }),
            log_of(|meter| clone_backing::reserve_blocks_copy_and_cleanup(sorted_key, meter)),
            log_of(|meter| clone_backing::reserve_blocks_copy_and_cleanup(sorted_value, meter)),
        ]
        .concat();
        assert_eq!(log, expected);
    }
}
