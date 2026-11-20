// See models/src/main/scala/coop/rchain/models/ParMapTypeMapper.scala

use super::par_map::ParMap;
use super::rholang::sorter::par_sort_matcher::ParSortMatcher;
use super::rholang::sorter::sortable::Sortable;
use crate::rhoapi::{EMap, KeyValuePair, Par};

pub struct ParMapTypeMapper;

impl ParMapTypeMapper {
    /// The value `emap` holds for the sorted form of `key`. Every map is built
    /// canonical, so its keys are already in sorted form; when two of them
    /// share it, the map answers with the later one.
    pub fn lookup(emap: &EMap, key: &Par) -> Option<Par> {
        let query = ParSortMatcher::sort_match(key).term;
        emap.kvs
            .iter()
            .rfind(|kv| kv.key.as_ref() == Some(&query))
            .map(|kv| kv.value.clone().expect("EMap entry has a value"))
    }

    /// Whether `emap` has a key with the same sorted form as `key`.
    pub fn contains_key(emap: &EMap, key: &Par) -> bool {
        let query = ParSortMatcher::sort_match(key).term;
        emap.kvs.iter().any(|kv| kv.key.as_ref() == Some(&query))
    }

    pub fn emap_to_par_map(emap: EMap) -> ParMap {
        let ps: Vec<(Par, Par)> = emap.kvs.into_iter().map(Self::unzip).collect();
        ParMap::new(ps, emap.connective_used, emap.locally_free, emap.remainder)
    }

    pub fn par_map_to_emap(par_map: ParMap) -> EMap {
        let kvs: Vec<KeyValuePair> = par_map
            .ps
            .sorted_list
            .into_iter()
            .map(|(k, v)| Self::zip(k, v))
            .collect();

        EMap {
            kvs,
            locally_free: par_map.locally_free,
            connective_used: par_map.connective_used,
            remainder: par_map.remainder,
        }
    }

    fn unzip(kvp: KeyValuePair) -> (Par, Par) { (kvp.key.unwrap(), kvp.value.unwrap()) }

    fn zip(k: Par, v: Par) -> KeyValuePair {
        KeyValuePair {
            key: Some(k),
            value: Some(v),
        }
    }
}
