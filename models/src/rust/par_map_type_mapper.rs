// See models/src/main/scala/coop/rchain/models/ParMapTypeMapper.scala

use std::collections::HashMap;

use prost::Message;

use super::par_map::ParMap;
use super::rholang::sorter::par_sort_matcher::ParSortMatcher;
use super::rholang::sorter::score_tree::compare_score_trees;
use super::rholang::sorter::sortable::Sortable;
use crate::rhoapi::{EMap, KeyValuePair, Par};

pub struct ParMapTypeMapper;

impl ParMapTypeMapper {
    /// The sorted value that `emap_to_par_map(emap)` holds for the sorted form
    /// of `key`. Raw keys that are equal keep the first key and the last value;
    /// among keys with the same sorted form, the map keeps the one that sorts
    /// last by score and then by encoded raw key.
    pub fn lookup(emap: &EMap, key: &Par) -> Option<Par> {
        let query = ParSortMatcher::sort_match(key).term;

        let mut positions: HashMap<&Par, usize> = HashMap::with_capacity(emap.kvs.len());
        let mut entries: Vec<(&Par, &Par)> = Vec::with_capacity(emap.kvs.len());
        for kv in &emap.kvs {
            let k = kv.key.as_ref().expect("EMap entry has a key");
            let v = kv.value.as_ref().expect("EMap entry has a value");
            match positions.get(k) {
                Some(&position) => entries[position].1 = v,
                None => {
                    positions.insert(k, entries.len());
                    entries.push((k, v));
                }
            }
        }

        entries
            .into_iter()
            .filter_map(|(k, v)| {
                let sorted_key = ParSortMatcher::sort_match(k);
                (sorted_key.term == query).then_some((sorted_key.score, k, v))
            })
            .max_by(|(a_score, a_key, _), (b_score, b_key, _)| {
                compare_score_trees(a_score, b_score)
                    .then_with(|| a_key.encode_to_vec().cmp(&b_key.encode_to_vec()))
            })
            .map(|(_, _, v)| ParSortMatcher::sort_match(v).term)
    }

    /// Whether a key of `emap` has the same sorted form as `key`.
    pub fn contains_key(emap: &EMap, key: &Par) -> bool {
        let query = ParSortMatcher::sort_match(key).term;
        emap.kvs.iter().any(|kv| {
            ParSortMatcher::sort_match(kv.key.as_ref().expect("EMap entry has a key")).term == query
        })
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
