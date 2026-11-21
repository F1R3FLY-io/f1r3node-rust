// See models/src/main/scala/coop/rchain/models/ParMapTypeMapper.scala

use super::par_map::ParMap;
use super::rholang::sorter::ordering::Ordering;
use super::rholang::sorter::par_sort_matcher::ParSortMatcher;
use super::rholang::sorter::score_tree::{compare_score_trees, ScoreAtom, Tree};
use super::rholang::sorter::sortable::Sortable;
use super::utils::union;
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

    /// Maps the sorted form of `key` to the sorted form of `value` in the
    /// canonical entries `kvs`, replacing any value it had.
    pub fn insert(kvs: &mut Vec<KeyValuePair>, key: &Par, value: &Par) {
        let sorted = Ordering::sort_key_value_pair(key, value);
        let (key, value) = sorted.term;
        let entry = Self::zip(key, value);
        match Self::search(kvs, &sorted.score) {
            Ok(position) => kvs[position] = entry,
            Err(position) => kvs.insert(position, entry),
        }
    }

    /// Removes the entry whose key has the same sorted form as `key` from the
    /// canonical entries `kvs`.
    pub fn remove(kvs: &mut Vec<KeyValuePair>, key: &Par) {
        if let Ok(position) = Self::search(kvs, &ParSortMatcher::sort_match(key).score) {
            kvs.remove(position);
        }
    }

    /// The canonical entries of both; where both have a key, the value from
    /// `other`.
    pub fn union(kvs: Vec<KeyValuePair>, other: Vec<KeyValuePair>) -> Vec<KeyValuePair> {
        Ordering::union_sorted(kvs, other, Self::key_score)
    }

    /// The canonical entries of `kvs` whose keys `other` does not have.
    pub fn diff(kvs: Vec<KeyValuePair>, other: &[KeyValuePair]) -> Vec<KeyValuePair> {
        Ordering::diff_sorted(kvs, other, Self::key_score, Self::key_score)
    }

    /// A map of the canonical entries `kvs`, with the flags they imply.
    pub fn canonical_emap(kvs: Vec<KeyValuePair>) -> EMap {
        let pars = || kvs.iter().flat_map(|kv| kv.key.iter().chain(kv.value.iter()));
        let locally_free = pars().fold(Vec::new(), |acc, p| union(acc, p.locally_free.clone()));
        let connective_used = pars().any(|p| p.connective_used);
        EMap {
            kvs,
            locally_free,
            connective_used,
            remainder: None,
        }
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

    fn key_score(kv: &KeyValuePair) -> Tree<ScoreAtom> {
        ParSortMatcher::sort_match(kv.key.as_ref().expect("EMap entry has a key")).score
    }

    fn search(kvs: &[KeyValuePair], score: &Tree<ScoreAtom>) -> Result<usize, usize> {
        kvs.binary_search_by(|kv| compare_score_trees(&Self::key_score(kv), score))
    }

    fn unzip(kvp: KeyValuePair) -> (Par, Par) { (kvp.key.unwrap(), kvp.value.unwrap()) }

    fn zip(k: Par, v: Par) -> KeyValuePair {
        KeyValuePair {
            key: Some(k),
            value: Some(v),
        }
    }
}
