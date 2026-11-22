// See models/src/main/scala/coop/rchain/models/SortedParMap.scala

use std::collections::HashMap;

use super::rholang::sorter::ordering::Ordering;
use crate::rhoapi::Par;

#[derive(Clone, Debug)]
pub struct SortedParMap {
    pub ps: HashMap<Par, Par>,
    pub sorted_list: Vec<(Par, Par)>,
}

impl SortedParMap {
    pub fn create_from_map(map: HashMap<Par, Par>) -> Self {
        let sorted_list = Ordering::sort_map(&map);

        SortedParMap {
            ps: map,
            sorted_list,
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

    pub fn keys(&self) -> Vec<Par> { self.sorted_list.iter().map(|kv| kv.0.clone()).collect() }

    pub fn values(&self) -> Vec<Par> { self.sorted_list.iter().map(|kv| kv.1.clone()).collect() }

    pub fn equals(&self, that: SortedParMap) -> bool { self.sorted_list == that.sorted_list }

    pub fn is_empty(&self) -> bool { self.ps.is_empty() && self.sorted_list.is_empty() }
}

impl IntoIterator for SortedParMap {
    type Item = (Par, Par);
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter { self.sorted_list.into_iter() }
}

impl PartialEq for SortedParMap {
    fn eq(&self, other: &Self) -> bool { self.sorted_list == other.sorted_list }
}
